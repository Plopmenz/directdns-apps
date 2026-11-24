# DirectDNS

A small Yggdrasil peer VPN app with a big on/off switch. Rust core and native
egui UI on desktop; Android uses a Kotlin `VpnService` shell around the same
Rust core via JNI. No webview.

## Design

- **Core:** `yggdrasil-mesh` 0.1 (`yggdrasil-rs`), pinned in `Cargo.lock` and
  isolated behind `crates/ygg-core` so it can be swapped for `yggdrasil-go` +
  gomobile if the spike fails.
- **UI:** `eframe`/`egui` 0.37 (`MIT OR Apache-2.0`) on Linux/desktop.
- **Peers:** `peer.yggdrasil.openmesh.cloud` is resolved over the normal
  network. Every resolved A/AAAA record becomes a `tls://<ip>:9003` peer. No
  SNI is needed because the peers accept raw-IP TLS.
- **DNS:** the same resolved IPs are set as plain DNS servers on port 53, for
  all domains. Requests leave via the normal network, not through Yggdrasil.
- **Routing:** split tunnel. Only `200::/7` is routed through the Yggdrasil
  interface; everything else keeps the existing default route.
- **Key:** a 32-byte seed is generated on first start and stored as
  `seed.key` (mode `0600` on Unix) in the platform data directory. The derived
  address is shown as `ip:<addr>` and copied to the clipboard on click.

## Layout

```
crates/ygg-core/    seed, peer resolution, node/TUN lifecycle, routing, Session API
crates/app-ui/      eframe desktop UI
crates/android/     JNI cdylib
platforms/android/  Gradle project, VpnService and minimal native Activity
scripts/            build helpers
```

## Nix builds

The flake builds the Linux binary, the Android JNI libraries, a debug-signed
APK, and a signed Android App Bundle for Google Play. It pins the Android SDK to
API 36, build-tools 36.0.0 and NDK 27c, and uses `rust-overlay` so the Android
Rust std targets are available.

```sh
nix build .#linux          # $out/bin/directdns
nix build .#android-native # $out/jniLibs/{arm64-v8a,armeabi-v7a} (JNI + tun-rs libs)
nix build .#android-apk    # $out/directdns-debug.apk
./scripts/build-aab.sh     # increments versionCode, builds signed AAB
nix build                  # all artifacts under $out (AAB only when keys are supplied)
nix run .                  # launch the Linux UI
nix develop                # toolchain, Android SDK/NDK, cargo-ndk, gradle
```

Use `./scripts/build-aab.sh` for Play releases. It increments `versionCode` in
`android-version.txt`, stages the file, then builds the AAB. `versionName` is
manual and currently `1.0.0`; edit it only when you want a new user-facing
version. A plain `nix build .#android-aab` uses the values currently in the file
without incrementing, but it is impure and requires `DIRECTDNS_KEY_DIR`; use the
script. `versionCode` starts at `0` so the first script run produces Play's
version code `1`.

Android outputs are provided on `x86_64-linux`; recent NDKs only publish x86_64
Linux host binaries. The APK and AAB are assembled hermetically with `aapt2`,
`kotlinc`, `d8`, `zipalign`/`bundletool` and `apksigner`/`jarsigner`; Gradle/Maven
is only needed for the optional local development flow below. All Nix outputs
are built in the sandbox.

`directdns-debug.apk` is called `debug` because it is signed with a generated
debug key for local sideloading. Use `directdns-release.aab` for Google Play.
The Android native libraries are `libdirectdns_android.so` (the Rust JNI bridge)
and `libtun_rs.so` (the TUN implementation), built for both `arm64-v8a` and
`armeabi-v7a`.

## Play upload key

The `keys/` directory is gitignored. `./scripts/build-aab.sh` reads it locally
and passes it to Nix via `DIRECTDNS_KEY_DIR`; a plain pure `nix build` will not
include the AAB. The local files shipped in this workspace were generated for
development. For a real Play release, replace them with your own upload key,
back them up, and keep them stable for future uploads:

PKCS#12 uses one password for both the keystore and the key, so only
`upload-password.txt` is needed. Do not set separate `-storepass` and
`-keypass` values.

```sh
mkdir -p keys
printf 'upload' >keys/key-alias.txt
printf 'your-upload-password' >keys/upload-password.txt
nix shell nixpkgs#jdk -c keytool -genkeypair \
  -keystore keys/upload.jks \
  -storetype PKCS12 \
  -storepass "$(cat keys/upload-password.txt)" \
  -alias "$(cat keys/key-alias.txt)" \
  -dname 'CN=Plopmenz DirectDNS Upload,O=Plopmenz,C=US' \
  -keyalg RSA -keysize 2048 -validity 10000 -noprompt
```

## Linux / desktop

TUN creation and route/DNS changes need root. `resolvectl` (systemd-resolved)
and `ip` must be installed.

```sh
cargo build -p app-ui --release
sudo RUST_LOG=info target/release/app-ui
```

Turn on with the big button. The address chip copies `ip:<addr>`. On shutdown,
or when turning off, the `200::/7` route and DNS override are reverted.

To sanity-check the split tunnel while connected:

```sh
ip -6 route show 200::/7
resolvectl status | grep -A3 DirectDNS   # or: resolvectl dns
dig +short example.com                    # plain DNS to peer IP, not via Yggdrasil
ping6 <a-yggdrasil-address>              # traffic via the tunnel
```

## Android

The Play application ID is `com.plopmenz.directdns`. Use the same package name
when creating the app in Play Console.

Hermetic Android builds are done through Nix; no separate `cargo-ndk` wrapper is
needed:

```sh
nix build .#android-native
nix build .#android-apk
nix develop   # optional local Gradle/JDK tooling
```

The Gradle files remain available for optional local development, for example
`gradle -p platforms/android assembleDebug` inside `nix develop`.

The Kotlin shell:

1. asks `YggBridge` for the persistent seed and derived address,
2. asks for the resolved DNS servers,
3. builds a `VpnService` with `addAddress(address, 128)`,
   `addRoute("200::", 7)`, and `addDnsServer(...)` for each peer IP,
4. calls `establish()`, detaches the TUN fd and hands it to Rust,
5. polls `YggBridge.status()` for the peers-up indicator.

The Activity is intentionally minimal native Android UI. It can later be
replaced by an `eframe` Android activity without changing the Rust core.

There is no custom launcher icon yet: `android:icon` is unset, so Android uses
its default launcher icon. The foreground notification small icon is Android's
built-in `android.R.drawable.ic_menu_manage`. Add branded drawables before the
Play release if you want a distinct identity.

## Tests

```sh
cargo test -p ygg-core
cargo check --workspace
cargo run -p ygg-core --example check   # connects to peers without needing root
```

The `check` example starts the core with the TUN disabled and prints peer status
until a link comes up. It verified 2/4 peers up against the live public peers.
`ygg-core` also includes a live lookup test for the hardcoded peer host and a
test that maps resolved peer IPs one-to-one to DNS servers.

## Current limitations

- The Rust core is very new (0.1.0). It is pinned and isolated; production use
  needs longer interop/soak testing.
- Linux DNS setup assumes systemd-resolved. NetworkManager-only systems will
  need a different configurator.
- Key storage is a plain `0600` file. Platform keystore/keyring integration is
  future work.
- iOS/macOS/Windows are not implemented yet.
- No custom Android launcher or notification icon yet.

## License

MIT. Copyright (c) 2026 Plopmenz. See [LICENSE](LICENSE).
