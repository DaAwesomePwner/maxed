# Third-party notices

Maxed ports capture, stream and protocol logic from RealmShark, distributed under MIT. The original notice follows unchanged.


The MIT License (MIT)

Copyright (c) 2022

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## UnityPy

`crates/maxed-assets` ports Unity serialized-file parsing logic derived from UnityPy (via RealmShark's resextractor), distributed under the MIT license. The original notice follows unchanged.

MIT License

Copyright (c) 2019-2026 K0lb3

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## P5 alert audio

`app/src-tauri/resources/sounds/white_bag.wav` was supplied by the owner from
RealmShark/Tomato, under its MIT license. Original SHA-256:
`429c4d316d6269a8b66e97698d560f0fde01fb48dbd5ab794208ec5215e897f6`.
The bundled copy is loudness-normalized by `scripts/normalize-alert-sounds.py`;
its SHA-256 is `3a075e4485d343d4ed656b96f8b82a6be485b5c50e9a09a52e8d21fcfef00a2e`.
The owner explicitly authorized this file, including its 1.497-second duration.
The original notice follows verbatim.


The MIT License (MIT)

Copyright (c) 2022

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

The three generated cues (Personal Best, Flawless Run and Key Popped) were
created for Maxed by `scripts/generate-alert-sounds.py` and are released under this repository's MIT license. See the sound README for
formulas, formats and SHA-256 hashes.

Audio backend: rodio 0.22.2 and cpal 0.17.3 (MIT OR Apache-2.0); WAV/PCM decoder:
Symphonia 0.5.5 (MPL-2.0). Notification dependency: tauri-plugin-notification 2.5.1
(MIT OR Apache-2.0), notify-rust 4.18.1 (MIT OR Apache-2.0),
tauri-winrt-notification 0.8.1 (MIT). The notification plugin is not registered
and production notification delivery is disabled pending installed-app acceptance.

Save dialog dependency: tauri-plugin-dialog 2.8.1 (Apache-2.0 OR MIT), with
tauri-plugin-fs 2.6.0 (Apache-2.0 OR MIT) and rfd 0.16.0 (MIT). It is used from Rust only for Stats CSV
export; the WebView has no dialog or file-system permission.
