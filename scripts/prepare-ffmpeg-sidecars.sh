#!/usr/bin/env bash
set -euo pipefail

FFMPEG_VERSION="8.1.2"
FFMPEG_SHA256="464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c"
X264_COMMIT="b35605ace3ddf7c1a5d67a2eb553f034aef41d55"
X264_SHA256="cd71a7515b0e9a012e1ac9b1f8415bebcaf6fc97d4db32286642ac4c0fbe24f9"
LAME_VERSION="3.101"
LAME_SHA256="7578af6eebd578b2bd64e468fac4ae1f03670a7e028166e67f855674b9b6aeac"
GNUTLS_VERSION="3.8.13"
GNUTLS_SHA256="ffed8ec1bf09c2426d4f14aae377de4753b53e537d685e604e99a8b16ca9c97e"
NETTLE_VERSION="3.10.2"
NETTLE_SHA256="fe9ff51cb1f2abb5e65a6b8c10a92da0ab5ab6eaf26e7fc2b675c45f1fb519b5"
MACOS_DEPLOYMENT_TARGET="11.0"
BUILD_ID="$FFMPEG_VERSION-x264-${X264_COMMIT:0:12}-lame-$LAME_VERSION-gnutls-$GNUTLS_VERSION-nettle-$NETTLE_VERSION-network-avfoundation-macos-$MACOS_DEPLOYMENT_TARGET-static-v5"

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_triple="${TAURI_ENV_TARGET_TRIPLE:-$(rustc --print host-tuple)}"

case "$target_triple" in
  aarch64-apple-darwin)
    target_arch="arm64"
    ;;
  x86_64-apple-darwin)
    target_arch="x86_64"
    ;;
  *)
    echo "FFmpeg sidecars are currently bundled only for macOS; skipping $target_triple."
    exit 0
    ;;
esac

cache_dir="$root_dir/.cache/ffmpeg"
archive="$cache_dir/ffmpeg-$FFMPEG_VERSION.tar.xz"
source_dir="$cache_dir/ffmpeg-$FFMPEG_VERSION"
x264_archive="$cache_dir/x264-$X264_COMMIT.tar.gz"
x264_source_dir="$cache_dir/x264-$X264_COMMIT"
x264_build_dir="$cache_dir/build-x264-$target_triple"
x264_install_dir="$cache_dir/install-x264-$target_triple"
lame_archive="$cache_dir/lame-$LAME_VERSION.tar.gz"
lame_source_dir="$cache_dir/lame-$LAME_VERSION"
lame_build_dir="$cache_dir/build-lame-$target_triple"
lame_install_dir="$cache_dir/install-lame-$target_triple"
nettle_archive="$cache_dir/nettle-$NETTLE_VERSION.tar.gz"
nettle_source_dir="$cache_dir/nettle-$NETTLE_VERSION"
nettle_build_dir="$cache_dir/build-nettle-$target_triple"
nettle_install_dir="$cache_dir/install-nettle-$target_triple"
gnutls_archive="$cache_dir/gnutls-$GNUTLS_VERSION.tar.xz"
gnutls_source_dir="$cache_dir/gnutls-$GNUTLS_VERSION"
gnutls_build_dir="$cache_dir/build-gnutls-$target_triple"
gnutls_install_dir="$cache_dir/install-gnutls-$target_triple"
build_dir="$cache_dir/build-$target_triple"
binary_dir="$root_dir/src-tauri/binaries"
ffmpeg_output="$binary_dir/ffmpeg-$target_triple"
ffprobe_output="$binary_dir/ffprobe-$target_triple"
version_marker="$binary_dir/ffmpeg-$target_triple.version"

verify_archive() {
  local actual
  actual="$(openssl dgst -sha256 "$archive" | awk '{print $NF}')"
  if [[ "$actual" != "$FFMPEG_SHA256" ]]; then
    echo "FFmpeg archive checksum mismatch: expected $FFMPEG_SHA256, got $actual." >&2
    exit 1
  fi
}

verify_x264_archive() {
  local actual
  actual="$(openssl dgst -sha256 "$x264_archive" | awk '{print $NF}')"
  if [[ "$actual" != "$X264_SHA256" ]]; then
    echo "x264 archive checksum mismatch: expected $X264_SHA256, got $actual." >&2
    exit 1
  fi
}

verify_lame_archive() {
  local actual
  actual="$(openssl dgst -sha256 "$lame_archive" | awk '{print $NF}')"
  if [[ "$actual" != "$LAME_SHA256" ]]; then
    echo "LAME archive checksum mismatch: expected $LAME_SHA256, got $actual." >&2
    exit 1
  fi
}

verify_nettle_archive() {
  local actual
  actual="$(openssl dgst -sha256 "$nettle_archive" | awk '{print $NF}')"
  if [[ "$actual" != "$NETTLE_SHA256" ]]; then
    echo "Nettle archive checksum mismatch: expected $NETTLE_SHA256, got $actual." >&2
    exit 1
  fi
}

verify_gnutls_archive() {
  local actual
  actual="$(openssl dgst -sha256 "$gnutls_archive" | awk '{print $NF}')"
  if [[ "$actual" != "$GNUTLS_SHA256" ]]; then
    echo "GnuTLS archive checksum mismatch: expected $GNUTLS_SHA256, got $actual." >&2
    exit 1
  fi
}

validate_binary_architecture() {
  local binary="$1"
  local description
  description="$(file "$binary")"
  if [[ "$description" != *"$target_arch"* ]]; then
    echo "$binary has the wrong architecture: $description" >&2
    exit 1
  fi

  local dependencies
  dependencies="$(otool -L "$binary")"
  if [[ "$dependencies" == *"/opt/homebrew/"* || "$dependencies" == *"/usr/local/"* ]]; then
    echo "$binary contains a package-manager dependency:" >&2
    echo "$dependencies" >&2
    exit 1
  fi
}

validate_native_features() {
  local host_triple
  host_triple="$(rustc --print host-tuple)"
  if [[ "$host_triple" != "$target_triple" ]]; then
    if ! "$ffmpeg_output" -version >/dev/null 2>&1 || \
       ! "$ffprobe_output" -version >/dev/null 2>&1; then
      echo "Cross-built $target_triple sidecars; runtime checks are unavailable on $host_triple."
      return
    fi
    echo "Running $target_triple feature checks through the local compatibility layer."
  fi

  local build_info encoders filters muxers protocols devices smoke_dir
  build_info="$($ffmpeg_output -version 2>&1)"
  if [[ "$build_info" != *"--enable-gnutls"* ]]; then
    echo "Bundled FFmpeg was not built with GnuTLS." >&2
    exit 1
  fi
  if [[ "$build_info" == *"--enable-securetransport"* ]]; then
    echo "Bundled FFmpeg unexpectedly enables SecureTransport." >&2
    exit 1
  fi

  encoders="$($ffmpeg_output -hide_banner -encoders 2>/dev/null)"
  filters="$($ffmpeg_output -hide_banner -filters 2>/dev/null)"
  muxers="$($ffmpeg_output -hide_banner -muxers 2>/dev/null)"
  protocols="$($ffmpeg_output -hide_banner -protocols 2>/dev/null)"
  devices="$($ffmpeg_output -hide_banner -devices 2>/dev/null)"

  for encoder in libx264 libmp3lame aac pcm_s16be pcm_s24be; do
    if [[ "$encoders" != *"$encoder"* ]]; then
      echo "Bundled FFmpeg is missing required encoder: $encoder" >&2
      exit 1
    fi
  done

  for filter in ebur128 astats acompressor alimiter equalizer highpass lowpass testsrc2; do
    if [[ "$filters" != *"$filter"* ]]; then
      echo "Bundled FFmpeg is missing required filter: $filter" >&2
      exit 1
    fi
  done

  if ! grep -Eq "^[[:space:]]*E[[:space:]]+flv([[:space:]]|$)" <<< "$muxers"; then
    echo "Bundled FFmpeg is missing required muxer: flv" >&2
    exit 1
  fi

  for protocol in icecast http https tcp tls rtmp rtmps; do
    if ! grep -Eq "^[[:space:]]*$protocol$" <<< "$protocols"; then
      echo "Bundled FFmpeg is missing required protocol: $protocol" >&2
      exit 1
    fi
  done

  if ! grep -Eq "^[[:space:]]*D.*avfoundation" <<< "$devices"; then
    echo "Bundled FFmpeg is missing the required AVFoundation input device." >&2
    exit 1
  fi

  smoke_dir="$(mktemp -d "${TMPDIR:-/tmp}/rau-studio-ffmpeg.XXXXXX")"
  trap 'rm -rf "$smoke_dir"' RETURN
  "$ffmpeg_output" \
    -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=0.1" \
    -c:a pcm_s16be "$smoke_dir/smoke.aiff"
  "$ffprobe_output" \
    -v error -select_streams a:0 -show_entries stream=codec_name -of csv=p=0 \
    "$smoke_dir/smoke.aiff" | grep -q "pcm_s16be"

  "$ffmpeg_output" \
    -hide_banner -loglevel error \
    -f lavfi -i "color=c=black:s=128x128:d=0.25" \
    -f lavfi -i "sine=frequency=440:duration=0.25" \
    -map 0:v:0 -map 1:a:0 -c:v libx264 -c:a aac -pix_fmt yuv420p \
    -movflags +faststart "$smoke_dir/smoke.mp4"
  "$ffprobe_output" \
    -v error -select_streams v:0 -show_entries stream=codec_name -of csv=p=0 \
    "$smoke_dir/smoke.mp4" | grep -q "h264"

  "$ffmpeg_output" \
    -hide_banner -loglevel error \
    -f lavfi -i "sine=frequency=440:duration=0.25" \
    -f lavfi -i "testsrc2=size=72x128:rate=30:duration=0.25" \
    -map 1:v:0 -map 0:a:0 -c:v libx264 \
    -b:v 500k -maxrate 500k -bufsize 1000k \
    -c:a aac -ar 44100 -ac 2 -f flv \
    "$smoke_dir/smoke.flv"
  "$ffprobe_output" \
    -v error -select_streams v:0 -show_entries stream=codec_name -of csv=p=0 \
    "$smoke_dir/smoke.flv" | grep -q "h264"
  "$ffprobe_output" \
    -v error -select_streams a:0 -show_entries stream=codec_name,sample_rate,channels -of csv=p=0 \
    "$smoke_dir/smoke.flv" | grep -Eq "aac,44100,(2|stereo)"

  "$ffmpeg_output" \
    -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=0.25" \
    -c:a libmp3lame -b:a 128k "$smoke_dir/smoke.mp3"
  "$ffprobe_output" \
    -v error -select_streams a:0 -show_entries stream=codec_name -of csv=p=0 \
    "$smoke_dir/smoke.mp3" | grep -q "mp3"
  rm -rf "$smoke_dir"
  trap - RETURN
}

validate_sidecars() {
  validate_binary_architecture "$ffmpeg_output"
  validate_binary_architecture "$ffprobe_output"
  validate_native_features
}

mkdir -p "$cache_dir" "$binary_dir"

if [[ -x "$ffmpeg_output" && -x "$ffprobe_output" && -f "$version_marker" && \
      -f "$binary_dir/COPYING.FFMPEG-GPLv2" && -f "$binary_dir/COPYING.X264-GPLv2" && \
      -f "$binary_dir/COPYING.LAME-LGPLv2" && -f "$binary_dir/COPYING.GNUTLS-LGPLv2.1" && \
      -f "$binary_dir/COPYING.NETTLE-GPLv2" ]] && \
   [[ "$(<"$version_marker")" == "$BUILD_ID" ]]; then
  validate_sidecars
  echo "FFmpeg $FFMPEG_VERSION sidecars are ready for $target_triple."
  exit 0
fi

if [[ ! -f "$archive" ]]; then
  echo "Downloading FFmpeg $FFMPEG_VERSION source from ffmpeg.org..."
  curl --fail --location --show-error \
    "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" \
    --output "$archive"
fi
verify_archive

if [[ ! -f "$x264_archive" ]]; then
  echo "Downloading x264 $X264_COMMIT source from the GitHub mirror..."
  curl --fail --location --show-error \
    "https://codeload.github.com/mirror/x264/tar.gz/$X264_COMMIT" \
    --output "$x264_archive"
fi
verify_x264_archive

if [[ ! -f "$lame_archive" ]]; then
  echo "Downloading LAME $LAME_VERSION source from SourceForge..."
  curl --fail --location --show-error \
    "https://downloads.sourceforge.net/project/lame/lame/$LAME_VERSION/lame-$LAME_VERSION.tar.gz" \
    --output "$lame_archive"
fi
verify_lame_archive

if [[ ! -f "$nettle_archive" ]]; then
  echo "Downloading Nettle $NETTLE_VERSION source from GNU..."
  curl --fail --location --show-error \
    "https://ftp.gnu.org/gnu/nettle/nettle-$NETTLE_VERSION.tar.gz" \
    --output "$nettle_archive"
fi
verify_nettle_archive

if [[ ! -f "$gnutls_archive" ]]; then
  echo "Downloading GnuTLS $GNUTLS_VERSION source from GnuPG..."
  curl --fail --location --show-error \
    "https://www.gnupg.org/ftp/gcrypt/gnutls/v3.8/gnutls-$GNUTLS_VERSION.tar.xz" \
    --output "$gnutls_archive"
fi
verify_gnutls_archive

if [[ ! -d "$source_dir" ]]; then
  tar -xf "$archive" -C "$cache_dir"
fi
if [[ ! -d "$x264_source_dir" ]]; then
  tar -xf "$x264_archive" -C "$cache_dir"
fi
if [[ ! -d "$lame_source_dir" ]]; then
  tar -xf "$lame_archive" -C "$cache_dir"
fi
if [[ ! -d "$nettle_source_dir" ]]; then
  tar -xf "$nettle_archive" -C "$cache_dir"
fi
if [[ ! -d "$gnutls_source_dir" ]]; then
  tar -xf "$gnutls_archive" -C "$cache_dir"
fi

rm -rf "$x264_build_dir" "$x264_install_dir"
mkdir -p "$x264_build_dir" "$x264_install_dir"

x264_configure_flags=(
  "--prefix=$x264_install_dir"
  "--host=$target_triple"
  "--enable-static"
  "--enable-pic"
  "--disable-cli"
  "--disable-opencl"
  "--extra-cflags=-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET"
  "--extra-ldflags=-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET"
)

echo "Building x264 $X264_COMMIT for $target_triple..."
x264_configure_log="$x264_build_dir/configure.log"
if ! (
  cd "$x264_build_dir"
  CC=clang MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" \
    "$x264_source_dir/configure" "${x264_configure_flags[@]}" > "$x264_configure_log" 2>&1
); then
  cat "$x264_configure_log" >&2
  exit 1
fi

jobs="$(sysctl -n hw.logicalcpu 2>/dev/null || echo 4)"
x264_build_log="$x264_build_dir/build.log"
if ! make -s -C "$x264_build_dir" -j "$jobs" > "$x264_build_log" 2>&1; then
  tail -n 300 "$x264_build_log" >&2
  exit 1
fi
make -s -C "$x264_build_dir" install-lib-static >> "$x264_build_log" 2>&1

if [[ ! -f "$x264_install_dir/lib/libx264.a" || ! -f "$x264_install_dir/lib/pkgconfig/x264.pc" ]]; then
  echo "x264 static library installation is incomplete." >&2
  exit 1
fi

rm -rf "$lame_build_dir" "$lame_install_dir"
mkdir -p "$lame_build_dir" "$lame_install_dir"

lame_configure_flags=(
  "--prefix=$lame_install_dir"
  "--host=$target_triple"
  "--enable-static"
  "--disable-shared"
  "--disable-frontend"
  "--disable-decoder"
)

echo "Building LAME $LAME_VERSION for $target_triple..."
lame_configure_log="$lame_build_dir/configure.log"
if ! (
  cd "$lame_build_dir"
  CC=clang \
    CFLAGS="-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
    LDFLAGS="-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
    MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" \
    "$lame_source_dir/configure" "${lame_configure_flags[@]}" > "$lame_configure_log" 2>&1
); then
  cat "$lame_configure_log" >&2
  exit 1
fi

lame_build_log="$lame_build_dir/build.log"
if ! make -s -C "$lame_build_dir" -j "$jobs" > "$lame_build_log" 2>&1; then
  tail -n 300 "$lame_build_log" >&2
  exit 1
fi
make -s -C "$lame_build_dir" install >> "$lame_build_log" 2>&1

if [[ ! -f "$lame_install_dir/lib/libmp3lame.a" || ! -f "$lame_install_dir/lib/pkgconfig/lame.pc" ]]; then
  echo "LAME static library installation is incomplete." >&2
  exit 1
fi

rm -rf "$nettle_build_dir" "$nettle_install_dir"
mkdir -p "$nettle_build_dir" "$nettle_install_dir"

nettle_configure_flags=(
  "--prefix=$nettle_install_dir"
  "--host=$target_triple"
  "--enable-static"
  "--disable-shared"
  "--enable-mini-gmp"
  "--disable-documentation"
  "--disable-openssl"
  "--disable-fat"
)

echo "Building Nettle $NETTLE_VERSION with mini-gmp for $target_triple..."
nettle_configure_log="$nettle_build_dir/configure.log"
if ! (
  cd "$nettle_build_dir"
  CC=clang \
    CFLAGS="-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
    LDFLAGS="-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
    MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" \
    "$nettle_source_dir/configure" "${nettle_configure_flags[@]}" > "$nettle_configure_log" 2>&1
); then
  cat "$nettle_configure_log" >&2
  exit 1
fi

nettle_build_log="$nettle_build_dir/build.log"
if ! make -s -C "$nettle_build_dir" -j "$jobs" > "$nettle_build_log" 2>&1; then
  tail -n 300 "$nettle_build_log" >&2
  exit 1
fi
make -s -C "$nettle_build_dir" install >> "$nettle_build_log" 2>&1

if [[ ! -f "$nettle_install_dir/lib/libnettle.a" || \
      ! -f "$nettle_install_dir/lib/libhogweed.a" || \
      ! -f "$nettle_install_dir/lib/pkgconfig/nettle.pc" || \
      ! -f "$nettle_install_dir/lib/pkgconfig/hogweed.pc" ]]; then
  echo "Nettle static library installation is incomplete." >&2
  exit 1
fi

rm -rf "$gnutls_build_dir" "$gnutls_install_dir"
mkdir -p "$gnutls_build_dir" "$gnutls_install_dir"

gnutls_configure_flags=(
  "--prefix=$gnutls_install_dir"
  "--host=$target_triple"
  "--enable-static"
  "--disable-shared"
  "--disable-doc"
  "--disable-tools"
  "--disable-cxx"
  "--disable-tests"
  "--disable-nls"
  "--disable-libdane"
  "--disable-hardware-acceleration"
  "--disable-rpath"
  "--with-nettle-mini"
  "--with-included-libtasn1"
  "--with-included-unistring"
  "--without-idn"
  "--without-p11-kit"
  "--without-tpm"
  "--with-tpm2=no"
  "--without-zlib"
  "--without-brotli"
  "--without-zstd"
)

echo "Building GnuTLS $GNUTLS_VERSION for $target_triple..."
gnutls_configure_log="$gnutls_build_dir/configure.log"
if ! (
  cd "$gnutls_build_dir"
  CC=clang \
    CFLAGS="-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
    LDFLAGS="-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET" \
    PKG_CONFIG_PATH="$nettle_install_dir/lib/pkgconfig" \
    MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" \
    "$gnutls_source_dir/configure" "${gnutls_configure_flags[@]}" > "$gnutls_configure_log" 2>&1
); then
  cat "$gnutls_configure_log" >&2
  exit 1
fi

gnutls_build_log="$gnutls_build_dir/build.log"
if ! make -s -C "$gnutls_build_dir" -j "$jobs" > "$gnutls_build_log" 2>&1; then
  tail -n 300 "$gnutls_build_log" >&2
  exit 1
fi
make -s -C "$gnutls_build_dir" install >> "$gnutls_build_log" 2>&1

if [[ ! -f "$gnutls_install_dir/lib/libgnutls.a" || \
      ! -f "$gnutls_install_dir/lib/pkgconfig/gnutls.pc" ]]; then
  echo "GnuTLS static library installation is incomplete." >&2
  exit 1
fi

rm -rf "$build_dir"
mkdir -p "$build_dir"

configure_flags=(
  "--prefix=/rau-studio/ffmpeg"
  "--extra-version=rau-studio"
  "--arch=$target_arch"
  "--target-os=darwin"
  "--cc=clang"
  "--extra-cflags=-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET -I$lame_install_dir/include -I$gnutls_install_dir/include -I$nettle_install_dir/include"
  "--extra-ldflags=-arch $target_arch -mmacosx-version-min=$MACOS_DEPLOYMENT_TARGET -L$lame_install_dir/lib -L$gnutls_install_dir/lib -L$nettle_install_dir/lib"
  "--disable-autodetect"
  "--disable-debug"
  "--disable-doc"
  "--disable-shared"
  "--enable-static"
  "--disable-programs"
  "--enable-ffmpeg"
  "--enable-ffprobe"
  "--enable-gpl"
  "--enable-libx264"
  "--enable-libmp3lame"
  "--enable-gnutls"
  "--enable-avfoundation"
  "--pkg-config-flags=--static"
  "--enable-audiotoolbox"
  "--enable-videotoolbox"
)

if [[ "$(rustc --print host-tuple)" != "$target_triple" ]]; then
  configure_flags+=("--enable-cross-compile")
fi

echo "Configuring FFmpeg $FFMPEG_VERSION for $target_triple..."
configure_log="$build_dir/configure.log"
if ! (
  cd "$build_dir"
  PKG_CONFIG_PATH="$x264_install_dir/lib/pkgconfig:$lame_install_dir/lib/pkgconfig:$gnutls_install_dir/lib/pkgconfig:$nettle_install_dir/lib/pkgconfig" \
    MACOSX_DEPLOYMENT_TARGET="$MACOS_DEPLOYMENT_TARGET" \
    "$source_dir/configure" "${configure_flags[@]}" > "$configure_log" 2>&1
); then
  cat "$configure_log" >&2
  exit 1
fi

jobs="$(sysctl -n hw.logicalcpu 2>/dev/null || echo 4)"
echo "Building ffmpeg and ffprobe with $jobs jobs..."
build_log="$build_dir/build.log"
if ! make -s -C "$build_dir" -j "$jobs" ffmpeg ffprobe > "$build_log" 2>&1; then
  tail -n 300 "$build_log" >&2
  exit 1
fi

install -m 755 "$build_dir/ffmpeg" "$ffmpeg_output"
install -m 755 "$build_dir/ffprobe" "$ffprobe_output"
strip -x "$ffmpeg_output" "$ffprobe_output"
install -m 644 "$source_dir/COPYING.GPLv2" "$binary_dir/COPYING.FFMPEG-GPLv2"
install -m 644 "$x264_source_dir/COPYING" "$binary_dir/COPYING.X264-GPLv2"
install -m 644 "$lame_source_dir/COPYING" "$binary_dir/COPYING.LAME-LGPLv2"
install -m 644 "$gnutls_source_dir/COPYING.LESSERv2" "$binary_dir/COPYING.GNUTLS-LGPLv2.1"
install -m 644 "$nettle_source_dir/COPYINGv2" "$binary_dir/COPYING.NETTLE-GPLv2"

validate_sidecars
printf '%s\n' "$BUILD_ID" > "$version_marker"
echo "FFmpeg $FFMPEG_VERSION sidecars are ready for $target_triple."
