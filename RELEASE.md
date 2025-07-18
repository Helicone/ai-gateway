These are instructions on how to conduct a release of the Helicone ai-gateway

1) Create a release branch branch:
    - `git checkout -b release/<version_tag>` eg `git checkout -b release/v0.2.0-beta.25`
2) Bump the version in the workspace `Cargo.toml`
    - eg with sed: `sed -i '' "/^\[workspace\.package\]/,/^\[/ s/^version = \"[^\"]*\"/version = \"<version_tag>\"/" Cargo.toml`
    - then ensure `Cargo.lock` is updated as well: `cargo c` (or let rust analyzer do it)
3) Generate the `CHANGELOG.md` updates, eg:

```sh
git fetch --tags
# the tag argument here is the new version to be released
git cliff --unreleased --tag v0.2.0-beta.29 --prepend CHANGELOG.md
git add CHANGELOG.md Cargo.*
```

4) Commit the changes and create a PR on Github:

```
git commit -m "release: v<version_tag>"
git push
gh pr create
```

5) Once the PR is merged in to `main`, pull your changes and tag the commit and push that tag
```sh
git checkout main
git pull
git tag v<version_tag>
git push --tags
```

done!