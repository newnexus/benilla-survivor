#!/usr/bin/env bash
# Build the Android APK: the game as libsurvivor_android.so (cargo, Android NDK), then the APK
# around it (Gradle). Uses the Android SDK, NDK, Java and Gradle that ship inside Unity's Android
# support, so nothing else needs installing. Run from Git Bash: crates/survivor-android/build-apk.sh
# The APK lands in app/build/outputs/apk/release/app-release.apk.
set -euo pipefail

UNITY_ANDROID="${UNITY_ANDROID:-C:/Program Files/Unity/Hub/Editor/6000.3.7f1/Editor/Data/PlaybackEngines/AndroidPlayer}"
NDK="$UNITY_ANDROID/NDK"
BIN="$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin"
ABI="${ABI:-arm64-v8a}"
case "$ABI" in
    arm64-v8a) TRIPLE=aarch64-linux-android ;;
    x86_64) TRIPLE=x86_64-linux-android ;;
    *) echo "ABI must be arm64-v8a or x86_64" >&2; exit 1 ;;
esac
TARGET_ENV="$(echo "$TRIPLE" | tr 'a-z-' 'A-Z_')"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$HERE/../.."

# 1. The game library.
export ANDROID_NDK_HOME="$NDK" ANDROID_NDK_ROOT="$NDK"
export "CC_${TRIPLE//-/_}=$BIN/${TRIPLE}26-clang.cmd"
export "CXX_${TRIPLE//-/_}=$BIN/${TRIPLE}26-clang++.cmd"
export "AR_${TRIPLE//-/_}=$BIN/llvm-ar.exe"
export "CARGO_TARGET_${TARGET_ENV}_LINKER=$BIN/${TRIPLE}26-clang.cmd"
(cd "$ROOT" && cargo build --release --target "$TRIPLE" -p survivor-android)

# 2. Into the APK's library folder, with the C++ runtime Bevy links against.
# One ABI per APK: an old build's other ABI must not ride along.
rm -rf "$HERE/app/src/main/jniLibs"
LIBS="$HERE/app/src/main/jniLibs/$ABI"
mkdir -p "$LIBS"
# Stripped: the debug info triples the APK.
"$BIN/llvm-strip.exe" --strip-debug -o "$LIBS/libsurvivor_android.so" \
    "$ROOT/target/$TRIPLE/release/libsurvivor_android.so"
cp "$NDK/toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/$TRIPLE/libc++_shared.so" "$LIBS/"

# 3. The game data, carried in the APK as assets/Data and unpacked by the game on its first
# launch: the packed Tirisfall data (survivor-pack's output, patch.MPQ + survivor/). Game data:
# it lives under app/build, never in git.
DATA="${DATA:-$HOME/Dropbox/TirisfallSurvivor/PC/Data}"
[ -f "$DATA/patch.MPQ" ] && [ -f "$DATA/survivor/spawns.tsv" ]     || { echo "No packed data at $DATA (set DATA=<folder with patch.MPQ and survivor/>)" >&2; exit 1; }
STAGE="$HERE/app/build/survivor-assets/Data"
mkdir -p "$STAGE/survivor"
cp -u "$DATA/patch.MPQ" "$STAGE/"
cp -u "$DATA"/survivor/*.tsv "$STAGE/survivor/"
# The quest givers' spoken lines (survivor-data with $VOICEOVER_DIR), when the data has them.
if [ -d "$DATA/survivor/voice" ]; then mkdir -p "$STAGE/survivor/voice"; cp -u "$DATA"/survivor/voice/* "$STAGE/survivor/voice/"; fi

# 4. The APK.
[ -f "$HERE/local.properties" ] || echo "sdk.dir=${UNITY_ANDROID/:/\\:}/SDK" > "$HERE/local.properties"
export JAVA_HOME="$UNITY_ANDROID/OpenJDK"
cd "$HERE"
"$JAVA_HOME/bin/java" -cp "$UNITY_ANDROID/Tools/gradle/lib/gradle-launcher-8.13.jar" \
    org.gradle.launcher.GradleMain --no-daemon assembleRelease
ls -la "$HERE/app/build/outputs/apk/release/app-release.apk"
