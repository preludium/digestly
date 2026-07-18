# Single multi-arch Docker image via buildx (ARM64 + x86-64)

**Status: accepted.**

Digestly targets both Raspberry Pi (ARM64) and typical x86-64 servers. A native single-arch build
on each host would require callers to pick arch-specific tags and splits the deploy story. The
base images (`node:20-bookworm-slim`, `rust:1.88-slim-bookworm`, `debian:bookworm-slim`) all
publish `linux/amd64` and `linux/arm64` variants, and the Dockerfile is architecture-agnostic -
both build stages compile for whatever platform they run on. This makes a single multi-arch image
via `docker buildx` the natural choice.

`docker compose up` on the target host still builds a single-arch image locally without any of
this complexity - the multi-arch build is a CI-level concern.

## Considered options

**Separate images per arch, tagged differently (e.g. `:latest-arm64`, `:latest-amd64`).** Callers
must pick the right tag manually; a deployer who pulls the wrong one gets a silent failure.
Rejected.

**Build only on the target host; no registry push.** Simpler for a single personal server, but
means no pre-built image is available for fresh machines or CI deployments. Rejected.

## Consequences

### One-off multi-arch build + push

```bash
# Create a buildx builder once (uses QEMU emulation for the non-native arch).
docker buildx create --name digestly --use
docker run --privileged --rm tonistiigi/binfmt --install all

# Build for both architectures and push a single multi-arch tag.
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t <registry>/digestly:latest \
  --push .
```

`--push` is required: a manifest list cannot be loaded into the local Docker image store, only
pushed to a registry (or written with `--output type=oci`).

### Build only for the local Pi

No buildx needed - the normal build targets the native arch:

```bash
docker compose build      # or: docker compose up --build
```

### CI release tags

`.github/workflows/docker-publish.yml` publishes multi-arch images to
`ghcr.io/preludium/digestly`:

| Trigger          | Tags produced                                 |
| ---------------- | --------------------------------------------- |
| Git tag `vX.Y.Z` | `X.Y.Z`, `X.Y`, `X`, `latest`, `sha-<short>` |
| Push to `main`   | `edge`, `sha-<short>`                         |

`latest` tracks the newest **release**, not the newest commit. Pin `X.Y.Z` (or `X.Y` for
patches) for a stable deploy; pull `edge` for unreleased `main`.

Cutting a release is a git tag on `main`:

```bash
git tag v1.2.3
git push origin v1.2.3
```

CI builds each architecture on a native runner (`ubuntu-latest` for AMD64 and
`ubuntu-24.04-arm` for ARM64), then assembles their pushed digests into one manifest. Buildx's
GitHub Actions cache is partitioned by architecture.
