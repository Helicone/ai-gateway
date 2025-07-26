create table public.provider_keys (
  id uuid not null default extensions.uuid_generate_v4 (),
  org_id uuid not null,
  provider_name text not null,
  provider_key_name text not null,
  vault_key_id uuid null,
  soft_delete boolean not null default false,
  created_at timestamp with time zone null default now(),
  provider_key text null,
  key_id uuid not null default (pgsodium.create_key ()).id,
  nonce bytea not null default pgsodium.crypto_aead_det_noncegen (),
  config jsonb null,
  constraint provider_keys_pkey primary key (id),
  constraint provider_keys_key_id_fkey foreign KEY (key_id) references pgsodium.key (id),
  constraint provider_keys_org_id_fkey foreign KEY (org_id) references organization (id)
) TABLESPACE pg_default;

create unique INDEX IF not exists org_provider_key_name_not_deleted_uniq on public.provider_keys using btree (org_id, provider_key_name) TABLESPACE pg_default
where
  (soft_delete = false);

create view public.decrypted_provider_keys as
select
  provider_keys.id,
  provider_keys.org_id,
  provider_keys.provider_name,
  provider_keys.provider_key_name,
  provider_keys.vault_key_id,
  provider_keys.soft_delete,
  provider_keys.created_at,
  provider_keys.provider_key,
  case
    when provider_keys.provider_key is null then null::text
    else case
      when provider_keys.key_id is null then null::text
      else convert_from(
        pgsodium.crypto_aead_det_decrypt (
          decode(provider_keys.provider_key, 'base64'::text),
          convert_to(provider_keys.org_id::text, 'utf8'::name),
          provider_keys.key_id,
          provider_keys.nonce
        ),
        'utf8'::name
      )
    end
  end as decrypted_provider_key,
  provider_keys.key_id,
  provider_keys.nonce,
  provider_keys.config
from
  provider_keys;


create trigger provider_keys_encrypt_secret_trigger_provider_key BEFORE INSERT
or
update OF provider_key on provider_keys for EACH row
execute FUNCTION provider_keys_encrypt_secret_provider_key ();

create trigger soft_delete_helicone_proxy_keys
after
update OF soft_delete on provider_keys for EACH row
execute FUNCTION soft_delete_helicone_proxy_keys ();