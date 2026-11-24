{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      androidVersionFile = builtins.readFile ./android-version.txt;
      androidVersionLines = lib.splitString "\n" androidVersionFile;
      androidVersionValue =
        key:
        let
          matches = builtins.filter (line: lib.hasPrefix "${key}=" line) androidVersionLines;
        in
        if matches == [ ] then
          throw "android-version.txt is missing ${key}"
        else
          builtins.substring
            (builtins.stringLength key + 1)
            (builtins.stringLength (builtins.head matches))
            (builtins.head matches);
      androidVersionCode = builtins.fromJSON (androidVersionValue "versionCode");
      version = androidVersionValue "versionName";

      mkPkgs =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };

      forAllSystems = f: lib.genAttrs systems (system: f (mkPkgs system));

      source = lib.cleanSourceWith {
        src = ./.;
        filter =
          path: _type:
          let
            base = toString ./.;
            full = toString path;
            rel =
              if lib.hasPrefix (base + "/") full then
                lib.substring (builtins.stringLength base + 1) (builtins.stringLength full) full
              else
                full;
            ignored =
              rel == ".git"
              || lib.hasPrefix ".git/" rel
              || rel == "target"
              || lib.hasPrefix "target/" rel
              || rel == "result"
              || lib.hasPrefix "result/" rel
              || lib.hasPrefix "platforms/android/.gradle/" rel
              || lib.hasPrefix "platforms/android/app/build/" rel
              || lib.hasPrefix "platforms/android/app/src/main/jniLibs/" rel;
          in
          !ignored;
      };

      androidTargets = [
        "aarch64-linux-android"
        "armv7-linux-androideabi"
      ];

      # Google publishes host binaries for x86_64 Linux in all recent NDKs, so
      # Android outputs are available on x86_64-linux only.
      androidAvailable = pkgs: pkgs.stdenv.hostPlatform.system == "x86_64-linux";

      # Signing material stays outside Git. The build-aab script sets this to an
      # absolute path; without it the AAB package is a clear error derivation.
      uploadKeyEnv = builtins.getEnv "DIRECTDNS_KEY_DIR";
      uploadKeyDir =
        if uploadKeyEnv == "" then
          null
        else
          builtins.path {
            path = /. + uploadKeyEnv;
            name = "directdns-upload-key";
          };
      androidAabAvailable = pkgs: androidAvailable pkgs && uploadKeyDir != null;

      mkAndroidSdk =
        pkgs:
        (pkgs.androidenv.composeAndroidPackages {
          platformVersions = [ "36" ];
          buildToolsVersions = [ "36.0.0" ];
          includeCmake = false;
          includeEmulator = false;
          includeNDK = true;
          ndkVersion = "27.2.12479018";
          includeSources = false;
          includeSystemImages = false;
          useGoogleAPIs = false;
        }).androidsdk;

      mkArtifacts =
        pkgs:
        let
          rust = pkgs.rust-bin.stable.latest.default.override {
            targets = androidTargets;
          };
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };

          linux = rustPlatform.buildRustPackage {
            pname = "directdns";
            inherit version;
            src = source;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [
              "-p"
              "app-ui"
            ];
            doCheck = false;

            postInstall = ''
              ln -s app-ui $out/bin/directdns
            '';

            meta = {
              description = "DirectDNS desktop UI with a Yggdrasil peer VPN";
              mainProgram = "directdns";
              platforms = [
                "x86_64-linux"
                "aarch64-linux"
              ];
              license = lib.licenses.mit;
              maintainers = [ { name = "Plopmenz"; } ];
            };
          };

          androidSdk = mkAndroidSdk pkgs;
          androidSdkPath = "${androidSdk}/libexec/android-sdk";

          androidNative = rustPlatform.buildRustPackage {
            pname = "directdns-android-native";
            inherit version;
            src = source;
            cargoLock.lockFile = ./Cargo.lock;
            doCheck = false;

            nativeBuildInputs = [
              pkgs.cargo-ndk
              pkgs.jdk
            ];

            buildPhase = ''
              runHook preBuild
              export ANDROID_NDK_HOME="${androidSdkPath}/ndk-bundle"
              export ANDROID_SDK_ROOT="${androidSdkPath}"
              export JAVA_HOME="${pkgs.jdk}"
              cargo ndk \
                -P 26 \
                -t arm64-v8a \
                -t armeabi-v7a \
                -o jniLibs \
                build -p android-bridge --release --locked --offline
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r jniLibs $out/jniLibs
              runHook postInstall
            '';

            meta = {
              description = "JNI libraries for the DirectDNS Android app";
              platforms = [ "x86_64-linux" ];
              license = lib.licenses.mit;
              maintainers = [ { name = "Plopmenz"; } ];
            };
          };

          androidApk = pkgs.stdenv.mkDerivation {
            pname = "directdns-android";
            inherit version;
            src = source;

            nativeBuildInputs = [
              androidSdk
              pkgs.jdk
              pkgs.kotlin
              pkgs.zip
            ];

            # Build the APK directly with the SDK tools instead of Gradle:
            # no Maven downloads and no unsandboxed/no-chroot build.
            buildPhase = ''
              runHook preBuild
              export JAVA_HOME="${pkgs.jdk}"
              export PATH="${pkgs.jdk}/bin:${pkgs.kotlin}/bin:$PATH"

              ROOT=$PWD
              SDK="${androidSdkPath}"
              BT="$SDK/build-tools/36.0.0"
              ANDROID_JAR="$SDK/platforms/android-36/android.jar"
              KOTLIN="${pkgs.kotlin}"

              mkdir -p build/res-compiled build/gen build/classes-java build/dex build/apkroot/lib build/apk
              cp -r platforms/android/app/src/main/AndroidManifest.xml \
                platforms/android/app/src/main/res \
                platforms/android/app/src/main/java build/
              cp -r ${androidNative}/jniLibs/arm64-v8a build/apkroot/lib/
              cp -r ${androidNative}/jniLibs/armeabi-v7a build/apkroot/lib/

              "$BT/aapt2" compile --dir build/res -o build/res-compiled/res.zip
              "$BT/aapt2" link \
                -o build/apk/base.apk \
                -I "$ANDROID_JAR" \
                --manifest build/AndroidManifest.xml \
                -R build/res-compiled/res.zip \
                --java build/gen \
                --min-sdk-version 26 \
                --target-sdk-version 36 \
                --version-code ${toString androidVersionCode} \
                --version-name ${lib.escapeShellArg version} \
                --auto-add-overlay

              javac -source 8 -target 8 -Xlint:-options \
                -bootclasspath "$ANDROID_JAR" \
                -d build/classes-java $(find build/gen -name '*.java')
              kotlinc \
                -classpath "$ANDROID_JAR:build/classes-java" \
                -d build/app-kotlin.jar \
                $(find build/java -name '*.kt')
              jar cf build/app-java.jar -C build/classes-java .

              "$BT/d8" --lib "$ANDROID_JAR" \
                --output build/dex \
                build/app-java.jar build/app-kotlin.jar "$KOTLIN/lib/kotlin-stdlib.jar" \
                >build/d8.log 2>&1 || { cat build/d8.log; exit 1; }

              cp build/apk/base.apk build/apk/unsigned.apk
              (
                cd build/dex
                zip -q "$ROOT/build/apk/unsigned.apk" classes*.dex
              )
              (
                cd build/apkroot
                zip -q -r "$ROOT/build/apk/unsigned.apk" lib
              )

              "$BT/zipalign" -f -p 4 build/apk/unsigned.apk build/apk/aligned.apk

              keytool -genkeypair \
                -keystore build/debug.keystore \
                -storetype PKCS12 \
                -storepass android \
                -keypass android \
                -alias androiddebugkey \
                -dname 'CN=Android Debug,O=Android,C=US' \
                -keyalg RSA \
                -keysize 2048 \
                -validity 10000 \
                -noprompt
              "$BT/apksigner" sign \
                --ks build/debug.keystore \
                --ks-pass pass:android \
                --key-pass pass:android \
                --out build/apk/app-debug.apk \
                build/apk/aligned.apk
              "$BT/apksigner" verify build/apk/app-debug.apk >/dev/null
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp build/apk/app-debug.apk $out/directdns-debug.apk
              mkdir -p $out/nix-support
              echo "file binary-dist \"$out/directdns-debug.apk\"" >$out/nix-support/hydra-build-products
              runHook postInstall
            '';

            meta = {
              description = "DirectDNS Android APK (debug-signed)";
              platforms = [ "x86_64-linux" ];
              license = lib.licenses.mit;
              maintainers = [ { name = "Plopmenz"; } ];
            };
          };

          androidAab =
            if uploadKeyDir == null then
              pkgs.runCommand "directdns-android-aab-missing-key" { } ''
                echo "Set DIRECTDNS_KEY_DIR to an absolute path containing upload.jks, key-alias.txt and upload-password.txt" >&2
                exit 1
              ''
            else
              pkgs.stdenv.mkDerivation {
                pname = "directdns-android-aab";
            inherit version;
            src = source;

            nativeBuildInputs = [
              androidSdk
              pkgs.jdk
              pkgs.kotlin
              pkgs.zip
              pkgs.unzip
              pkgs.bundletool
            ];

            # Same Kotlin/Rust sources as the APK, packaged in Android App
            # Bundle format and signed with the Play upload key from DIRECTDNS_KEY_DIR.
            buildPhase = ''
              runHook preBuild
              export JAVA_HOME="${pkgs.jdk}"
              export PATH="${pkgs.jdk}/bin:${pkgs.kotlin}/bin:$PATH"

              ROOT=$PWD
              SDK="${androidSdkPath}"
              BT="$SDK/build-tools/36.0.0"
              ANDROID_JAR="$SDK/platforms/android-36/android.jar"
              KOTLIN="${pkgs.kotlin}"
              BUNDLETOOL="${pkgs.bundletool}/bin/bundletool"
              KEY_DIR="${uploadKeyDir}"

              UPLOAD_PASS=$(cat "$KEY_DIR/upload-password.txt")
              KEY_ALIAS=$(cat "$KEY_DIR/key-alias.txt")

              mkdir -p build/res-compiled build/gen build/classes-java build/dex \
                build/aapt-out build/module/manifest build/module/dex build/module/lib build/aab
              cp -r platforms/android/app/src/main/AndroidManifest.xml \
                platforms/android/app/src/main/res \
                platforms/android/app/src/main/java build/

              "$BT/aapt2" compile --dir build/res -o build/res-compiled/res.zip
              "$BT/aapt2" link \
                -o build/base-resources.zip \
                -I "$ANDROID_JAR" \
                --manifest build/AndroidManifest.xml \
                -R build/res-compiled/res.zip \
                --java build/gen \
                --min-sdk-version 26 \
                --target-sdk-version 36 \
                --version-code ${toString androidVersionCode} \
                --version-name ${lib.escapeShellArg version} \
                --auto-add-overlay \
                --proto-format

              javac -source 8 -target 8 -Xlint:-options \
                -bootclasspath "$ANDROID_JAR" \
                -d build/classes-java $(find build/gen -name '*.java')
              kotlinc \
                -classpath "$ANDROID_JAR:build/classes-java" \
                -d build/app-kotlin.jar \
                $(find build/java -name '*.kt')
              jar cf build/app-java.jar -C build/classes-java .

              "$BT/d8" --lib "$ANDROID_JAR" \
                --output build/dex \
                build/app-java.jar build/app-kotlin.jar "$KOTLIN/lib/kotlin-stdlib.jar" \
                >build/d8.log 2>&1 || { cat build/d8.log; exit 1; }

              # bundletool expects aapt2 proto-format output in this layout.
              unzip -q build/base-resources.zip -d build/aapt-out
              cp build/aapt-out/AndroidManifest.xml build/module/manifest/AndroidManifest.xml
              cp build/aapt-out/resources.pb build/module/resources.pb
              cp -r build/aapt-out/res build/module/res
              cp build/dex/classes*.dex build/module/dex/
              cp -r ${androidNative}/jniLibs/arm64-v8a build/module/lib/
              cp -r ${androidNative}/jniLibs/armeabi-v7a build/module/lib/

              (
                cd build/module
                zip -q -r "$ROOT/build/base-module.zip" .
              )

              "$BUNDLETOOL" build-bundle \
                --modules=build/base-module.zip \
                --output=build/aab/app-release-unsigned.aab \
                --overwrite

              cp build/aab/app-release-unsigned.aab build/aab/directdns-release.aab
              jarsigner \
                -keystore "$KEY_DIR/upload.jks" \
                -storetype PKCS12 \
                -storepass "$UPLOAD_PASS" \
                -digestalg SHA-256 \
                -sigalg SHA256withRSA \
                build/aab/directdns-release.aab \
                "$KEY_ALIAS"

              "$BUNDLETOOL" validate --bundle=build/aab/directdns-release.aab
              jarsigner -verify build/aab/directdns-release.aab >/dev/null
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp build/aab/directdns-release.aab $out/directdns-release.aab
              mkdir -p $out/nix-support
              echo "file binary-dist \"$out/directdns-release.aab\"" >$out/nix-support/hydra-build-products
              runHook postInstall
            '';

            meta = {
              description = "DirectDNS Android App Bundle (Play upload key)";
              platforms = [ "x86_64-linux" ];
              license = lib.licenses.mit;
              maintainers = [ { name = "Plopmenz"; } ];
            };
          };

          all = pkgs.runCommand "directdns-artifacts" { } ''
            mkdir -p $out/bin
            ln -s ${linux}/bin/directdns $out/bin/directdns-linux
            ${lib.optionalString (androidAvailable pkgs) ''
              mkdir -p $out/android
              ln -s ${androidNative}/jniLibs $out/android/jniLibs
              ln -s ${androidApk}/directdns-debug.apk $out/android/directdns-debug.apk
              ${lib.optionalString (androidAabAvailable pkgs) ''
                ln -s ${androidAab}/directdns-release.aab $out/android/directdns-release.aab
              ''}
            ''}
          '';
        in
        {
          inherit linux all;
        }
        // lib.optionalAttrs (androidAvailable pkgs) {
          "android-sdk" = androidSdk;
          android-native = androidNative;
          android-apk = androidApk;
          android-aab = androidAab;
        };
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          artifacts = mkArtifacts pkgs;
        in
        artifacts
        // {
          default = if artifacts.android-apk ? outPath then artifacts.all else artifacts.linux;
        }
      );

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${(mkArtifacts pkgs).linux}/bin/directdns";
          meta = {
            description = "Launch the DirectDNS Linux UI";
            maintainers = [ { name = "Plopmenz"; } ];
          };
        };
      });

      devShells = forAllSystems (
        pkgs:
        let
          rust = pkgs.rust-bin.stable.latest.default.override {
            targets = androidTargets;
          };
          androidSdk = mkAndroidSdk pkgs;
          androidSdkPath = "${androidSdk}/libexec/android-sdk";
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              pkgs.cargo-ndk
              pkgs.pkg-config
              pkgs.android-tools
            ]
            ++ lib.optionals (androidAvailable pkgs) [
              pkgs.gradle
              pkgs.jdk
            ];

            env = {
              ANDROID_HOME = lib.optionalString (androidAvailable pkgs) androidSdkPath;
              ANDROID_SDK_ROOT = lib.optionalString (androidAvailable pkgs) androidSdkPath;
            };
          };
        }
      );
    };
}
