# Copilot OpenAI Proxy

A small, local Rust proxy that exposes selected GitHub Copilot functionality through an OpenAI-compatible HTTP API.

> **Disclaimer:** This project is independent software. It is not affiliated with, endorsed by, or sponsored by GitHub, Microsoft, OpenAI, or any other service provider. Use it only with accounts and services you are authorized to use, and follow the applicable service terms.

## Features

- GitHub device-flow authentication, including GitHub Enterprise domains.
- OpenAI-compatible `chat/completions` and `responses` endpoints.
- OpenAI-compatible model discovery through `GET /v1/models`.
- Optional glob-based model filtering with `--agent_whitelist`.
- Loopback-only binding by default.
- Streaming upstream responses with JSONL request logging.

## Requirements

- Rust stable toolchain.
- An active GitHub Copilot subscription.
- Network access to GitHub during login and while forwarding requests.

## Quick start

Build and start the proxy:

```sh
cargo run --release
```

On first start, enter a GitHub Enterprise domain when prompted. Press **Enter** to use `github.com`. Complete the device login in a browser. Credentials are stored locally, and the proxy prints a local API key before listening on `127.0.0.1:4000`.

Verify the service with the printed key:

```sh
curl http://127.0.0.1:4000/v1/models \
  -H "Authorization: Bearer YOUR_LOCAL_API_KEY"
```

The same base URL and key can be used with OpenAI-compatible clients:

- Base URL: `http://127.0.0.1:4000/v1`
- API key: the generated local API key

## Command-line options

```text
Usage: copilot-openai-proxy [serve] [--shared] [--agent_whitelist=PATTERN]
```

| Option | Description |
| --- | --- |
| `serve` | Explicitly selects the server command. It is also the default behavior. |
| `--shared` | Binds to `0.0.0.0:4000` instead of loopback. Use only on a trusted network. |
| `--agent_whitelist=PATTERN` | Filters `/v1/models` by model ID. The default is `*`, which keeps every model. |
| `--help` / `-h` | Prints usage information. |

The whitelist uses simple glob matching:

- `*` matches any sequence of characters.
- `?` matches one character.
- Matching is case-sensitive.

For example, to expose only model IDs containing `oss`:

```sh
cargo run --release -- --agent_whitelist='*oss*'
```

The whitelist affects model discovery only. It does not rewrite model IDs or filter chat and response requests.

## API endpoints

| Method | Endpoint | Authentication | Description |
| --- | --- | --- | --- |
| `GET` | `/healthz` | None | Returns service health and version. |
| `GET` | `/v1/models` | Local bearer token | Lists available models after whitelist filtering. |
| `POST` | `/v1/chat/completions` | Local bearer token | Forwards chat-completion requests. |
| `POST` | `/v1/responses` | Local bearer token | Forwards Responses API requests. |

## Local data and logging

The proxy stores local state under `~/.copilot-openai-proxy/`:

- `auth.db` — GitHub OAuth data stored in SQLite.
- `api-token` — the generated local bearer token. On Unix systems, new token files use mode `0600`.

Request and response records are appended to `requests.log.jsonl` in the process working directory. Logs can contain prompts, responses, URLs, and other sensitive data. Do not commit or share them without review.

The default listener is loopback-only. `--shared` exposes the API to all network interfaces; protect the generated key and use an appropriate network boundary when enabling it. Upstream TLS certificate verification is enabled.

## Releases

Pushing a version tag that starts with `v` builds the release binary on `windows-latest` and creates a GitHub release with the Windows executable attached:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The asset is named `copilot-openai-proxy-windows-x86_64.exe`. The workflow uses the locked dependency versions from `Cargo.lock` and generates release notes from commits since the previous tag.

## Development

Format, test, lint, and package the project with:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo package --allow-dirty --no-verify
```

Contributions should preserve the documented API behavior, avoid committing generated files or credentials, and include focused tests for new observable behavior.

## License

This project is licensed under the [MIT License](LICENSE).

GitHub Copilot, GitHub, Microsoft, OpenAI, and related names and logos are trademarks of their respective owners.
