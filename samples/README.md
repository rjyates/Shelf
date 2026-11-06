# Local upload samples

Files here are for manual import testing against a local Shelf server. They are not required to build.

`.cbz` binaries stay gitignored (fetch locally). `.cbr` / `.djvu` fixtures below are committed so CBR/DjVu import can be exercised without an extra download step.

## Size caps (server ingest)

| Check | Limit |
| --- | --- |
| CBR archive size | 500 MiB |
| CBR total extracted bytes | 500 MiB |
| CBR single page / member | 50 MiB |
| DjVu file size | 200 MiB |
| Comic page count (after ZIP normalize) | 10 000 |
| Comic page image in packed ZIP | 50 MiB |

Over those limits, import returns 400 (“not a supported book file”). Encrypted or empty image archives also fail.

## `Barnum_Was_Right_1917.cbz`

- **What:** Three-page newspaper comic strip *Barnum Was Right* (1917).
- **Source:** [Internet Archive — Old Comic Strips - B](https://archive.org/details/bunglefamilythe191845) (`Barnum Was Right (1917).cbz`).
- **Rights:** US public-domain era (1917). The Archive item shipped a 7z payload mislabeled as CBZ; this file was re-packed as a real ZIP of PNGs so Shelf’s CBZ importer accepts it.
- **Try:** Add book → pick this `.cbz` (or `curl` it to `/api/v1/books/import` with `X-Shelf-Filename: Barnum_Was_Right_1917.cbz`).

## `Barnum_Was_Right_1917.cbr`

- **What:** Same three PNG pages as the CBZ, packed as RAR 5 (`.cbr`).
- **Source:** Re-packed from `Barnum_Was_Right_1917.cbz` (same IA strip / rights).
- **Try:** Add book → pick this `.cbr`. Catalogue format should be `cbr`; `/file` is a ZIP for Readium; pages use `cbzpage(N)`.

## `Goody_Two_Shoes.djvu`

- **What:** Multi-page DjVu scan of *The History of Little Goody Two-Shoes* (IA item).
- **Source:** [Internet Archive — goodytwoshoes00newyiala](https://archive.org/details/goodytwoshoes00newyiala) (`goodytwoshoes00newyiala.djvu`, ~1.0 MiB).
- **Rights:** Digitized public-domain edition (IA item).
- **Try:** Add book → pick this `.djvu`. Ingest renders pages to a ZIP of PNGs and stores format `djvu`.
