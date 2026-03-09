# foc-devnet

**Run a local Filecoin network with FOC (Filecoin Onchain Contracts) in minutes.**

A developer-friendly tool for spinning up complete Filecoin test networks with smart contract support, deterministic key generation, and automated deployment -- all running locally in rootless podman containers.

This is a fork of [FilOzone/foc-devnet](https://github.com/FilOzone/foc-devnet) rewritten to use podman natively instead of Docker. All container operations go through a `ContainerRunBuilder` abstraction that produces correct rootless podman args by construction (`--userns=keep-id`, `:z` bind mount labels, proper `HOME`/`GIT_CONFIG` env vars, no `-u` flag). SELinux is supported via udica policy generation.

---

## Quick Start

### Prerequisites

**Rootless podman**: no docker daemon or docker group needed.

```bash
podman --version
```

**Rust toolchain**: for building the CLI itself.

```bash
rustup --version   # or install from https://rustup.rs
```

**udica (Fedora/RHEL, SELinux enforcing)**: generates tailored SELinux policies for the containers. It depends on the system `selinux` python C extension, so it must come from the distro package manager -- pip/uv can't install it.

```bash
sudo dnf install -y udica
```

If SELinux is not enforcing (or you're on a non-SELinux distro), udica is not needed -- the init step will skip policy generation.

**host.docker.internal**: required for SP-to-SP fetch. Add to `/etc/hosts`:

```bash
echo '127.0.0.1 host.docker.internal' | sudo tee -a /etc/hosts
```

### Step 1: Initialize

```bash
cargo run -- init
```

This will:
- Download required repositories (or use your local ones)
- Build container images
- Generate deterministic cryptographic keys
- Generate and install SELinux policy (if enforcing)

**Using local repositories?** Specify them during init:

```bash
cargo run -- init \
    --curio local:/home/user/code/curio \
    --filecoin-services local:/home/user/code/filecoin-services \
    --lotus local:/home/user/code/lotus \
    --synapse-sdk local:/home/user/code/synapse-sdk \
    --force
```

### Step 2: Build

```bash
cargo run -- build lotus
cargo run -- build curio
```

Compiles Lotus and Curio binaries inside containers. Build artifacts are cached. Can run in parallel if you have the CPU/RAM for it.

### Step 3: Start the Network

```bash
cargo run -- start --parallel
```

This will:
- Create the genesis block
- Start Lotus daemon with FEVM enabled
- Deploy FOC smart contracts (including MockUSDFC)
- Start storage provider(s) with PDP support

Use `cargo run -- start` (without `--parallel`) if you hit issues -- sequential startup is slower but easier to debug.

### Step 4: Use the Network

All connection details (contract addresses, user keys, SP endpoints) are exported to `devnet-info.json`:

```bash
cat ~/.foc-devnet/state/latest/devnet-info.json
```

See [examples/README.md](examples/README.md) for more usage examples.

---

## Key Features

**Lean host requirements**: only needs podman, Rust, and tar. Everything else (Lotus, Curio, all dependencies) is built inside containers.

**Configurable repositories**: depends on 4 repos, all overridable with local paths:
- `filecoin-services` -- FOC smart contracts
- `curio` -- storage provider
- `lotus` -- Filecoin daemon
- `synapse-sdk` -- PDP verification

**Deterministic setup**: pinned versions, fixed-seed key generation, consistent state persisted per run in `~/.foc-devnet/run/<run-id>/`.

**Fully automated**: genesis creation, network init, contract deployment, SP registration -- no manual steps.

**Programmable**: contract addresses in JSON, step context for scripting, `FOC_DEVNET_BASEDIR` env var for custom base directory, `~/.foc-devnet/state/latest/` symlink to most recent run.

**Isolated networks**: podman user-defined networks separate nodes like production deployments.

**Built-in contracts**: MockUSDFC (ERC-20 test token), Multicall3 (batch calls).

---

## System Requirements

| Requirement | Details |
|-------------|---------|
| **Rust** | 1.70+ ([rustup.rs](https://rustup.rs)) |
| **Podman** | rootless mode, 4.0+ |
| **udica** | for SELinux policy generation (Fedora/RHEL only) |
| **tar** | archive utility (usually pre-installed) |
| **Disk Space** | ~20GB for images and chain data |

---

## More Documentation

See **[README_ADVANCED.md](README_ADVANCED.md)** for:
- All commands reference (init, build, start, stop, status, version)
- Configuration system (config.toml structure, parameters)
- Directory structure
- Container architecture and network topology
- Repository management and local repo linking
- Service provider configuration
- Troubleshooting

---

## Examples

See [examples/README.md](examples/README.md).

---

## License

MIT License -- see [LICENSE](LICENSE) file for details.

---

## Support

- **Issues**: [GitHub Issues](https://github.com/FilOzone/foc-devnet/issues)
