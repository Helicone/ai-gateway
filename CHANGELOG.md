# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0-beta.27] - 2025-07-08

### 🚀 Features

- Configurable retries (#208)

## [0.2.0-beta.26] - 2025-07-08

### 🚀 Features

- *(examples)* Add example for python tool calls with streaming (#198)

### 🐛 Bug Fixes

- Restore debug logs for stream errs (#199)
- Match anthropic tool_result expected role (#202)

### ⚙️ Miscellaneous Tasks

- General s3 clients (#204)
- Cleanup tower types for unified api, router (#205)
- Add default ./config.yaml path (#206)

## [0.2.0-beta.25] - 2025-07-06

### 🚀 Features

- Generic openai handler (#194)
- Add mistral support (#195)

### 🐛 Bug Fixes

- Remove docs for deprecated style of config (#196)

## [0.2.0-beta.24] - 2025-07-05

### ⚙️ Miscellaneous Tasks

- Rustup update and allow dirty on release workflow (#193)

## [0.2.0-beta.23] - 2025-07-04

### ⚙️ Miscellaneous Tasks

- Use newer ubuntu for rust 1.88 on release workflow (#192)

## [0.2.0-beta.22] - 2025-07-04

### ⚙️ Miscellaneous Tasks

- Rustup update on release workflows (#191)

## [0.2.0-beta.21] - 2025-07-04

### 🚀 Features

- [**breaking**] Rate-limiting with redis (#182)

### ⚙️ Miscellaneous Tasks

- Add named inference provider variant (#190)

## [0.2.0-beta.20] - 2025-07-03

### 🚀 Features

- Backwards compat helicone.features config (#189)

### 🐛 Bug Fixes

- Allow default model mappings in config yaml
- Correctly detect mime type in img mappers

### ⚙️ Miscellaneous Tasks

- Better trace logs
- Load env var for helicone key in examples

## [0.2.0-beta.18] - 2025-07-02

### 🚀 Features

- Add more benchmarks
- Setup db listener
- Redis cache benchmark

### 🐛 Bug Fixes

- Misleading err log when observ. disabled
- Propagate errors from streams
- Resolve EnvFilter clone compilation errors in telemetry
- *(app)* Replace hardcoded sleep with proper server ready signaling
- *(config)* Quote model names with colons in mapping
- *(config)* Isolate model mapping to prevent parsing errors
- *(catch_panic)* Address inefficient to_string call
- *(app)* Resolve type mismatches
- *(server)* Adjust startup sequence and fix telemetry shutdown
- Propagate errors from streams

### 🚜 Refactor

- *(config)* Improve configuration loading logic for Secret types
- Break down oversized main function into focused helpers
- Break down oversized App::new function into focused helper methods
- *(router)* Eliminate code duplication in PathAndQuery extraction
- *(error)* Replace generic Box<dyn Error> with specific error types

### 🎨 Styling

- Apply cargo fmt formatting
- Cargo fmt

### ⚙️ Miscellaneous Tasks

- Bump cargo deps
- Improve mock server config

## [0.2.0-beta.16] - 2025-07-01

### ⚙️ Miscellaneous Tasks

- Bump release

## [0.2.0-beta.15] - 2025-06-30

### 🚀 Features

- Rename with helicone prefix, terraform resources for flyio
- Add Prometheus production configuration and remove outdated Fly.io README
- Deploy all infra needed for load testing
- Enabled creation and destruction of fly resources
- Fly infra also creates the applications via terraform
- Add redis for cache support (#172)

### 🐛 Bug Fixes

- Remove machine creation from terraform resources (fly.toml)
- Removed coloring via peacock extension of settings.json

### 🚜 Refactor

- Remove unused fly machines
- Removed redis from flyio machine

### ⚙️ Miscellaneous Tasks

- Updated to latest gateway spec

## [0.2.0-beta.14] - 2025-06-27

### 🐛 Bug Fixes

- Map error responses to openai errors
- Wrap error response in error key

## [0.2.0-beta.13] - 2025-06-26

### 🚀 Features

- Don't require v1 in path

### 🐛 Bug Fixes

- LLM observability for cached responses
- Streams for mapped providers in unified api

### 📚 Documentation

- Add public beta shield (#163)

### ⚙️ Miscellaneous Tasks

- Fixing tests part 1
- Fixing tests part 2
- Fix test

### ◀️ Revert

- Extend_query

## [0.2.0-beta.11] - 2025-06-26

### 🐛 Bug Fixes

- Extend with query params (#160)

## [0.2.0-beta.10] - 2025-06-26

### 🚀 Features

- Inject v1 if not ther (#158)

### 🚜 Refactor

- Simplify stream chunks

### 📚 Documentation

- *(readme)* Updated content with the latest reviews
- *(readme)* Updated config.yaml based on new releases
- *(readme)* Fixed videos
- *(video)* Improve video

### ⚙️ Miscellaneous Tasks

- Bump to beta.10 (#159)

## [0.2.0-beta.9] - 2025-06-26

### 🚀 Features

- [ENG-2147] Terraform resources for the AI Gateway (#145)
- Render deploy (#144)
- Init benchmark dir (#149)
- Change default ip address in code

### 🐛 Bug Fixes

- Accept-encoding header issue

### 💼 Other

- Add AI gateway server address as a default var

### 📚 Documentation

- Add beta warning on benchmarks (#150)
- Added shield for status: public beta

## [0.2.0-beta.8] - 2025-06-25

### 🚀 Features

- Make it so that users don't need to pass in both ai/v1 just chat/completetions

### 🐛 Bug Fixes

- Use ranges based of num requests to fix test
- Remove export from .env.template
- Fix caching for POST requests
- Max-size kebab casing

### 🚜 Refactor

- Update log levels, messages
- Helicone-observability field -> helicone

### ⚙️ Miscellaneous Tasks

- Bump version (#146)

## [0.2.0-beta.6] - 2025-06-25

### 🚀 Features

- New rust CLI for testing
- Fly IO support (#133)
- Add version tag for --version
- Added pretty welcome messages and reduced log level of many logs

### 🐛 Bug Fixes

- Properly deserialize router names
- Sse streaming prepend with data
- Streaming test

### 📚 Documentation

- Remove helix name from everywhere

### ⚙️ Miscellaneous Tasks

- Add arm64 docker images

## [0.2.0-beta.5] - 2025-06-23

### 🚀 Features

- Add py & TS examples

## [0.2.0-beta.4] - 2025-06-23

### 🚀 Features

- Add warn log when running debug build

## [0.2.0-beta.3] - 2025-06-23

### 🚀 Features

- Health check (#120)
- Add self hosted runners (#122)
- Use redis 8 in ci job

### 🐛 Bug Fixes

- Update grafana dashboard JSON (#115)
- Map Provider name before sending to Jawn (#117)
- Remove protoc step (#121)
- Rename google -> gemini (#118)
- Skip rust ci when unchanged, concurrency limit
- Revert self hosted ci
- Dont crash if jawn is down
- Dont hang in integration test

### 📚 Documentation

- *(readme)* Updated links and snippets
- *(readme)* Fixed naming for ai-gateway and introduced discovery call
- *(readme)* Fixed empty bash command in demo.md

### ⚙️ Miscellaneous Tasks

- Update anthropic-ai-sdk to point upstream (#116)

## [0.2.0-beta.2] - 2025-06-20

### 🐛 Bug Fixes

- Don't err for valid anthropic streams (#114)

## [0.2.0-beta.1] - 2025-06-20

### 🚀 Features

- *(llm-obs)* Llm observ tests
- Add health check monitors for providers
- Replace std HashMap with rustc-hash FxHashMap for performance (#32)
- Add per helicone user rate limit (#34)
- Passthrough reqs for unsupported endpoints
- Add tower-otel-http-metrics
- *(metrics)* Add system level metrics
- *(metrics)* Add provider health metrics
- *(metrics)* Add request/resp count metrics
- *(metrics)* Add better error metrics
- *(metrics)* Add grafana dashboard
- Add viz for error_count and auth metrics
- *(deploy)* Add cargo-chef dockerfile
- Add providers configurations
- Added npm and brew distributions
- Use jemalloc for perf+lower memory usage
- Global and router-level RL configs w/optin
- Add ability to load test
- Use tower-governor for retry-* headers
- Configurable response headers
- Add Ollama provider support
- Setup commit hooks
- Add docker compose
- Rate Limit aware load balancing
- Set auth headers for websockets
- Improved configs + config validation (#72)
- Add github action for docker builds (#67)
- Direct proxy to provider based on URL (#74)
- Better config logging (#75)
- Unified API (#77)
- Track TFFT (#79)
- [ENG-1529] Bedrock Mapper (#60)
- Add LLM observability in sidecar mode (#82)
- Request and response caching (#93)
- Add sidecar yaml, instructions (#95)
- No key required to run gateway, check keys at runtime (#110)
- Better error handling on auth (#108)

### 🐛 Bug Fixes

- Valid model ids are when not mapping providers
- Add enable_control_plane config
- Bedrock headers signing (#92)
- Updated stubr ref (#94)
- Remove `Bearer` when checking api key hash (#96)
- Secret<_> serialize issue when merging configs (#109)
- S3 client issue not being constructed (#112)

### 💼 Other

- Fix path
- V0.2.0-beta.1 (#111)

### 🚜 Refactor

- More robust model id parsing
- Comprehensive embedded model configs
- Use global cp state, remove Mutex
- Better URLs for named routers (#73)
- Remove optin rate limit config (#81)
- Moved websocket mutex to rwlock (#84)
- Update config for launch, remove docs (#101)
- SelfHosted -> Sidecar deployment target (#106)

### ⚙️ Miscellaneous Tasks

- Remove postgres/db stuff
- Add mw to mark sensitive headers
- Code smell
- Clean up
- Basic demo
- Update deps
- Audit error handling
- Remove schema-filter crate (#78)
- Remove duplicate env var (#80)
- Updated npm package name to ai-gateway (#85)
- Update docker name (#86)
- Update toml contributors and links (#87)
- Get releases to work (#99)
- Fix cargo lock pinned rev (#100)
- Fix pre-release typo (#102)
- Remove unused config flag (#107)

<!-- generated by git-cliff -->
