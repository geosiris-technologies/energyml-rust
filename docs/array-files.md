# Where the arrays are looked for

An EPC does not hold its big numeric arrays. It holds XML objects, and each
array pointer names an external file, usually an HDF5 file, and a dataset path
inside it. The name is a string, and strings go wrong: someone renames
`model.epc` and `model.h5`, a colleague sends only the `.h5` under whatever name
their tool chose, a folder is moved. Every number is still on disk. Only the
name is lost.

So the library does not ask "which file has the right name" but **which file
holds this dataset**. The name decides the order of the search; the content ends
it.

## The order

For every dataset, the places are tried in this order, and the first file that
holds it wins. A file that exists but does not hold the dataset steps aside for
the next one.

| # | Place | Where it comes from |
|---|---|---|
| 1 | A file **pinned** to the name the document uses | `--h5 model.h5=path/to/renamed.h5` |
| 2 | A file **offered** without a name, in the order given | `--h5 path/to/data.h5` |
| 3 | The name the document **writes**, absolute or relative to the package's folder | the pointer and the `.rels` of its proxy |
| 4 | The file beside the package with the **package's name** | `model.epc` gives `model.h5` |
| 5 | **Any other file** in the package's folder, in a stable order | the folder, not its subfolders |

Two details:

- An offer comes before the document's own name, because offering a file is an
  instruction, and people give one precisely when the package is wrong.
- Another `.epc` in the folder is never a candidate: a folder holding two
  packages is the ordinary case, and one is not a place to look for the other's
  numbers.

A package whose names are correct is not searched: it resolves through its own
declaration, and nothing is said.

## What you are told

The search never goes somewhere unexpected in silence. What it says depends on
what it did:

- **A file found by the search (4 or 5)** is reported on stderr when the command
  ends, one line per file, with how many datasets came from it:

  ```text
  warning: 736 dataset(s) read from `folder/export.h5` instead of the file they point at (the package names `model.h5`): another file in the package's folder
  ```

  It counts datasets, not array pointers: two objects can point at the same one,
  and it is counted once. A file you pinned or offered is used as you said, and
  is not reported when it holds the data.

- **A `--h5` path that is not on disk** is reported at once, and ignored:

  ```text
  warning: --h5 `typo.h5` does not exist and is ignored
  ```

  It is dropped from the search because there is nothing to open, and a mistyped
  path used to look like a package with no data file.

- **Nothing holds a dataset**: `energyml arrays --values` says how many arrays
  could not be read on stderr, and each failing array names what was tried,
  including an offered path that is not there:

  ```text
  `/resqml22/…/values` is in none of the 0 candidate file(s) for `model.h5`; offered by the caller and not found on disk: `typo.h5`
  ```

  `arrays` still exits 0 when some arrays could not be read. A script that needs
  the arrays to be there should ask `energyml h5`, which exits 1 when a file is
  not found.

## The command that answers the question: `energyml h5`

```sh
energyml h5 model.epc                  # where each array's numbers really are
energyml h5 model.epc --fix fixed.epc  # write the answer into a package
```

`h5` prints one line per file the package names, with the file that was found
for it and why, or `NOT FOUND`, and exits 1 when a file is not found. `--fix`
writes a package whose declarations name the files the search found, so that
the next reader does not pay for the search again.

To rename on purpose, use `energyml rename`, which changes the fields, the
relationships and the file together.

## From the library

- `ExternalFiles::offer(path)` and `ExternalFiles::set(name, path)` (through
  `EpcStore::external_files_mut`) are `--h5`'s two forms.
- `EpcStore::array_departures()` lists, once the arrays have been read, the
  fallbacks the search took: the name the document wrote, the file read, why,
  and how many datasets came from it. Empty for a package that was right.
- `ExternalFiles::missing_offers()` lists the pinned and offered paths that are
  not on disk.
- `EpcStore::resolve_array(location)` says, for one array, which file holds it
  and what put that file in the search.

Which file was read is a fact the library knows and a face tells: the library
has no terminal. See also [Troubleshooting](troubleshooting.md) for the symptom
"arrays cannot be found".
