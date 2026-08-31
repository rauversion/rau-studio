# FFmpeg

Rau Studio for macOS includes the `ffmpeg` and `ffprobe` command-line programs
from FFmpeg 8.1.2. The bundled FFmpeg programs are built with the GPL-licensed
x264 encoder from commit `b35605ace3ddf7c1a5d67a2eb553f034aef41d55`, the
LGPL-licensed LAME 3.101 MP3 encoder, GnuTLS 3.8.13, Nettle 3.10.2 with
mini-gmp, network protocols, and macOS AVFoundation input support. TLS is
provided by the statically linked GnuTLS/Nettle stack rather than macOS
SecureTransport. GnuTLS is built with its included libtasn1 and libunistring
sources. The sidecars do not include non-free components.

This FFmpeg configuration and x264 are distributed under the GNU General
Public License version 2 or later. Nettle, mini-gmp, and the dual-licensed
included libunistring components are used under their GNU General Public
License version 2-or-later option. GnuTLS, its included libtasn1 sources, and
the remaining included libunistring components are distributed under the GNU
Lesser General Public License version 2.1 or later. LAME is distributed under
the GNU Lesser General Public License version 2. Their applicable license texts
are included beside this notice in the application resources.

Corresponding source code:

https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz

https://codeload.github.com/mirror/x264/tar.gz/b35605ace3ddf7c1a5d67a2eb553f034aef41d55

https://downloads.sourceforge.net/project/lame/lame/3.101/lame-3.101.tar.gz

https://ftp.gnu.org/gnu/nettle/nettle-3.10.2.tar.gz

https://www.gnupg.org/ftp/gcrypt/gnutls/v3.8/gnutls-3.8.13.tar.xz

The same source archives are attached to each Rau Studio release that
distributes these binaries. The exact configure options are recorded in
`scripts/prepare-ffmpeg-sidecars.sh`.
