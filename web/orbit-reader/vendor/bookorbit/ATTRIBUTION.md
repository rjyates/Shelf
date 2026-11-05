# Book Orbit reader sources

This tree is derived from [Book Orbit](https://github.com/bookorbit/bookorbit)
(`client/src/features/reader` including `pdf-v4`, Foliate assets, shared reader
UI, and supporting modules required to mount the EPUB and PDF readers).

Book Orbit is licensed under the GNU Affero General Public License v3.0 only.
Shelf is also AGPL-3.0-only. Retain this notice in copies and substantial
portions. Upstream project: https://github.com/bookorbit/bookorbit

Foliate (`public/assets/foliate` in the orbit-reader package) is from
[foliate-js](https://github.com/johnfactotum/foliate-js) as vendored by Book
Orbit; Foliate itself is MIT-licensed.

Shelf-specific adapters (API path rewrites, cookie sessions, local stubs for
bookmarks/annotations/preferences that Shelf does not store yet) live beside
these sources and are marked in-file.
