# Cloud runner OverlayBD

This opt-in connects a verified local converted image to a shared read-only
UBLK device, with an independent writable qcow2 for each box. Local mode imports complete blobs; registry mode reads layers on demand through
the external daemon. Neither mode converts images during box creation.
Ordinary SDK `NewRuntime`, CLI and `boxlite serve` keep the OCI pipeline even
when the environment variable is set. Only the explicit cloud runner constructor
can enable this backend; compile its native library with `CLOUD_RUNNER=1`.

## Fixture

Prepare a **native OverlayBD 0.1.0, single-platform Linux image** using the
[upstream converter](https://github.com/containerd/accelerated-container-image/tree/main/cmd/convertor).
Match the runner architecture. TurboOCI, tar layers, and image indexes are not
supported. A converter working directory alone is not an OCI layout: the input
must contain the converted manifest, OCI image configuration and every layer
under `blobs/sha256/<hex>`.

For an already converted registry image, export on any machine with registry
access using [skopeo copy](https://github.com/containers/skopeo/blob/main/docs/skopeo-copy.1.md):

```sh
# Replace the reference with the converted single-platform manifest digest.
export OVERLAYBD_TEST_IMAGE='registry.example.com/team/alpine-obd@sha256:CONVERTED_MANIFEST_DIGEST'
export BOXLITE_OVERLAYBD_IMAGE_DIR="$PWD/overlaybd-fixture"
skopeo copy --preserve-digests --dest-oci-accept-uncompressed-layers \
  "docker://$OVERLAYBD_TEST_IMAGE" "oci:$BOXLITE_OVERLAYBD_IMAGE_DIR:fixture"
```

Do not use the original OCI image digest. The converter changes its manifest.
Export must preserve native layer bytes and descriptor annotations; import
rejects missing annotations, wrong hashes, sizes, architecture or OS.
Transfer the entire layout to the test Linux machine. EC2 access is not needed
to prepare it; Linux VM/runner access is needed for the device/VM acceptance.

## Linux host and build

Use a kernel with `ublk_drv` (mainline Linux 6.0+ or a backport), KVM, and the
normal BoxLite runtime prerequisites. Install the upstream daemon and its
dependencies separately. This implementation targets the `/v1/list`, `/v1/add`
and `/v1/del` API at OverlayBD commit
[`29ace5a7f78be6eeab79fd0bac1b74d3445f4b89`](https://github.com/containerd/overlaybd/blob/29ace5a7f78be6eeab79fd0bac1b74d3445f4b89/src/ublk/ublkd_main.cpp#L173);
do not assume older release packages contain this daemon.

```sh
sudo modprobe ublk_drv
sudo systemctl start overlaybd-ublkd  # install the upstream unit first
make dev:go CLOUD_RUNNER=1
```

Run the cloud runner/test with access to the root-only daemon socket
`/var/run/overlaybd-ublk/ublkd.sock`, its `/dev/ublkbN` nodes, and `/usr/bin/curl`.
The daemon and runner must see the same absolute runtime-home paths and device
namespace. A container deployment must provide these explicitly; this PR does
not modify deployment privileges or install/start the daemon automatically.
For local mode, set `BOXLITE_OVERLAYBD_ENABLED=true` and
`BOXLITE_OVERLAYBD_IMAGE_DIR` for the runner. `BOXLITE_OVERLAYBD_SOURCE` defaults
to `local`. Production native packaging also accepts `make dist:go CLOUD_RUNNER=1`.

## Acceptance

Use a disposable Linux host with a converted image containing `/bin/sh`, `cat`
and `sleep`. Build the Go native library above, then run with the same operator
permissions as the runner:

```sh
make test:integration:overlaybd
```

The test creates its own runtime home and two boxes. It verifies one shared
device, independent writable files, continued I/O after one box stops, device
release after both stop, and preserved writes after restart/rebinding. It cleans
up its own boxes only. The target fails on non-Linux or missing fixture settings.

Unit tests exercise real Unix-socket HTTP with a simulated daemon, blob import,
backend selection and qcow2 headers. They do **not** establish kernel/device,
bubblewrap/Landlock or libkrun compatibility. Real Linux acceptance is required
before enabling this in a deployed runner.

## Lifetime and limits

Verified blobs are cached under `<home>/overlaybd/blobs`; source layout removal
does not invalidate an existing box, including after runtime restart. The source
directory is required only when importing a new image. Each image's device config
has no writable upper layer. The kernel read-only flag and capacity are checked
before using a node as backing. Box stops release references; the last reference
deletes the device. Runtime recovery retains devices owned by surviving shims
and reclaims only its own unused device configs. If reconciliation fails,
runtime initialization fails without stopping recovered shims; restore daemon
access and retry initialization. Keep the daemon alive while
boxes run; a vanished device is an error, not a transparent replacement.

Clone, export and snapshot mutations remain rejected for OverlayBD boxes.
Disabled/local runtimes cannot freshly start them. Keep a cloud build available
to reclaim devices after disabling the opt-in. Blob cache GC and automatic daemon recovery are deferred. A changed device number requires
an atomic qcow2 copy/rebind; filesystems without reflink may incur a disk copy.

## Remote metadata preparation (PR4-A)

The `cloud-runner` Rust build exposes `OverlaybdImages::new(home).pull_metadata(reference,
registries).await`. It fetches only a digest-pinned single-platform manifest and
its image config, using the existing registry transport and authentication settings.
Each metadata body is limited to 4 MiB; authentication and fetching have a combined
30-second deadline. Layers are not downloaded or marked as fully verified.

The result includes the image config, full layer descriptors and a stable candidate
`repo_blob_url`. Raw verified manifest/config bytes are atomically published together
under `<home>/overlaybd/metadata/<manifest-digest-hex>/`. This cache is separate from
local complete blobs and the ordinary OCI image index. A corrupt existing entry
fails validation; concurrent identical pulls can reuse the published entry.

This preparation API alone does not enable remote Box creation, bind an image
source or change Runner configuration.
Daemon credentials are independent of Rust metadata credentials; token exchange
results and temporary redirect URLs are never persisted as the image origin.


## Remote Rust runtime (PR4-B)

Linux builds with `cloud-runner` can explicitly call
`BoxliteRuntime::new_cloud_runner_registry(options)`. `options.image_registries`
authorizes manifest/config requests; provision daemon layer credentials separately.
HTTPS verification is required (`skip_verify` is rejected); explicitly configured
HTTP registries are supported. The existing `new_cloud_runner(options, image_dir)`
keeps its local-import/disabled semantics. Go exposes `NewCloudRunnerRegistry`,
backed by the additive C ABI `boxlite_cloud_runner_registry_runtime_new`.
Existing C and Go constructors keep their signatures and behavior.

Before saving a Box, the runtime binds its manifest digest to one origin in the
SQLite `overlaybd_source` table. The first successful binding wins; local/remote
or registry/repository changes for the same digest fail, even if every Box is
stopped. A failed Box save can leave a reusable binding. Origin records contain
no credentials or redirect URLs and are not automatically deleted or replaced.

Schema v11 marks PR3 OverlayBD images for local verification, including boxes
created but never started. Cloud initialization verifies complete local blobs and
any existing device config before committing that origin. Missing/corrupt bindings
after migration fail closed; runtime initialization never guesses a replacement.
Old Box starts use the stored origin, independent of the new runtime's default.
Turning OverlayBD off still prevents fresh starts. Recovery failures leave surviving
shims alone.

Remote starts read the verified metadata cache and pass `repoBlobUrl` and native
`digest`/`size` lowers to the daemon. They reuse PR3 read-only shared UBLK devices,
per-Box qcow2 files and lease cleanup. Metadata validation does not mean full-layer
SHA256 verification; registry trust and the daemon's format checks remain required.
Missing metadata and layer-access errors fail rather than falling back to OCI.

Use the verified daemon profile: `cacheType: "ocf"`, `cacheSizeGB: 1` and
`download.enable: false`; log actual layer Range bytes from a cold cache.
The pinned daemon's `file` cache keys by layer digest and can retain the first
source across repositories, including failed/auth-denied sources. It is not a
supported profile for this remote runtime; OCF preserves source isolation.
Provision OCF before enabling remote creation; changing a running daemon's cache
requires draining its Boxes first, since live devices are not transparently replaced. This
limit covers the daemon cache, not local blobs, metadata or per-Box qcow2 files.
Deployment must separately budget those directories and keep the daemon alive.
Do not infer complete offline availability from a warm cache.

## Runner registry mode (PR4-C)

Set `BOXLITE_OVERLAYBD_ENABLED=true`, `BOXLITE_OVERLAYBD_SOURCE=registry`, and
leave `BOXLITE_OVERLAYBD_IMAGE_DIR` unset. With OverlayBD enabled, unknown sources,
missing local directories and registry mode combined with a local directory fail
at Runner startup. With the opt-in disabled, source/directory settings are ignored
and new Boxes use OCI. Ordinary SDK/CLI entry points always use OCI.

| Entry / build | New Box backend |
| --- | --- |
| Ordinary SDK/CLI, or disabled Runner | OCI |
| Enabled Runner, source unset or `local`, directory supplied | Local OverlayBD |
| Enabled Runner, source `registry`, no directory | Remote OverlayBD |
| Enabled OverlayBD without Linux or `cloud-runner` native build | Error |

Runner reads these options when constructing its single runtime. Changing the
process environment does not update an existing runtime. After a restart, new
Boxes use the configured default; existing Boxes retain their persisted backend
and origin. Keep the daemon running for existing OverlayBD Boxes. OCI Boxes
continue using OCI even after enabling OverlayBD.

Runner's existing GHCR/Docker Hub credentials authorize metadata requests;
`INSECURE_REGISTRIES` selects explicit HTTP. Configure matching layer access in
the daemon's native credential provider separately; Runner does not copy tokens
into daemon configuration. Use the pinned daemon and OCF profile above. Provision
and test credentials/cache before enabling registry mode; there is no OCI fallback
on metadata, daemon or layer errors.

For real remote acceptance, push an externally converted, single-platform image
and select its manifest digest, provision daemon credentials, then run:

```sh
export BOXLITE_OVERLAYBD_SOURCE=registry
unset BOXLITE_OVERLAYBD_IMAGE_DIR
export OVERLAYBD_TEST_IMAGE='registry.example.com/team/image@sha256:CONVERTED_MANIFEST_DIGEST'
# Provide OVERLAYBD_TEST_REGISTRY_USERNAME / PASSWORD through a secure environment
# for private metadata access. Only for an explicit HTTP test registry:
# export OVERLAYBD_TEST_HTTP=true
make test:integration:overlaybd
```

This reuses the local acceptance lifecycle checks through the remote Go/C entry.
For a cold cache, record registry Range bytes and verify that startup transfers
less than all layers with daemon background download disabled. A successful warm
start alone does not prove lazy fetching. Image conversion and uploaded image
assets remain external to this repository.
