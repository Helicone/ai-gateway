create table public.routers (
  id uuid not null default gen_random_uuid (),
  created_at timestamp with time zone not null default now(),
  organization_id uuid not null,
  name character varying null,
  hash character varying(255) not null,
  constraint routers_pkey primary key (id),
  constraint routers_hash_key unique (hash),
  constraint public_routers_organization_id_fkey foreign KEY (organization_id) references organization (id) on update CASCADE on delete CASCADE
) TABLESPACE pg_default;


create table public.router_config_versions (
  id uuid not null default gen_random_uuid (),
  created_at timestamp with time zone not null default now(),
  router_id uuid not null,
  version character varying(255) not null,
  config jsonb not null,
  constraint router_config_versions_pkey primary key (id),
  constraint public_router_config_versions_router_id_fkey foreign KEY (router_id) references routers (id) on update CASCADE on delete CASCADE
) TABLESPACE pg_default;

CREATE OR REPLACE FUNCTION broadcast_router_config_change() RETURNS trigger AS $$
DECLARE
  org_id uuid;
  router_hash varchar(255);
BEGIN
  -- Get organization_id from the router join
  SELECT r.organization_id, r.hash INTO org_id, router_hash
  FROM routers r
  WHERE r.id = COALESCE(NEW.router_id, OLD.router_id);

  PERFORM pg_notify(
    'connected_cloud_gateways',           -- channel
    json_build_object(
      'event', 'router_config_updated',
      'organization_id', org_id,
      'router_hash', router_hash,
      'router_config_id', COALESCE(NEW.id, OLD.id),
      'config', COALESCE(NEW.config, OLD.config),
      'version', COALESCE(NEW.version, OLD.version),
      'router_id', COALESCE(NEW.router_id, OLD.router_id),
      'op', TG_OP
    )::text
  );
  RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

create trigger t_connected_gateways_broadcast
after INSERT
or DELETE
or
update on router_config_versions for EACH row
execute FUNCTION broadcast_router_config_change ();


