# Local upload samples

Files here are for manual import testing against a local Shelf server. They are not required to build.

## `Barnum_Was_Right_1917.cbz`

- **What:** Three-page newspaper comic strip *Barnum Was Right* (1917).
- **Source:** [Internet Archive — Old Comic Strips - B](https://archive.org/details/bunglefamilythe191845) (`Barnum Was Right (1917).cbz`).
- **Rights:** US public-domain era (1917). The Archive item shipped a 7z payload mislabeled as CBZ; this file was re-packed as a real ZIP of PNGs so Shelf’s CBZ importer accepts it.
- **Try:** Add book → pick this `.cbz` (or `curl` it to `/api/v1/books/import` with `X-Shelf-Filename: Barnum_Was_Right_1917.cbz`).
