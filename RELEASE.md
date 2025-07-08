These are instructions on how to conduct a release of the Helicone ai-gateway

1) Create a branch bumping the version of `Cargo.toml` and `./ai-gateway/Cargo.toml`
```sh
git checkout -b release-<version_tag>
# Either run the next line OR update the Cargo.toml file manually
sed -i '' "/^\[workspace\.package\]/,/^\[/ s/^version = \"[^\"]*\"/version = \"<version_tag>\"/" Cargo.toml
cargo build # makes sure that your Cargo.lock updates - rust-analyzer should do this though

git cliff --unreleased --tag <version_tag> --prepend CHANGELOG.md
git add CHANGELOG.md Cargo.*
git commit -m "release: v<version_tag>"
git push
gh pr create
```

2) Once merged in on main pull your changes and tag the commit and push that tag
```sh
git checkout main
git pull
git tag v<version_tag>
git push --tags
```

done!