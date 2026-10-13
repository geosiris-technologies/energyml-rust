# 0013 - WITSML channels in HDF5

Status: accepted

## Context

WITSML 2.1 describes the bulk data of a log in a `ChannelData` structure: either
an inline data blob in the XML or a URI of an external file (the reference
format named in release notes is not HDF5). Large logs do not fit comfortably in
XML, while RESQML and the rest of an EPC already keep large arrays in HDF5 beside
the package.

## Decision

Supporting HDF5 for channel data is an **extension of the standard**, and is
treated as one:

- the external file is named in `FileUri` as a relative URI, as the standard
  intends, and is the same string as the file of every other array in the package;
- the path of the dataset inside that file goes in `Data`;
- the form is recognisable without ambiguity, and a reader that does not know the
  extension **refuses** rather than misreads;
- the writer produces it **only on request**; the default stays values in the XML.

## Consequences

- Large logs can live with the other arrays in one HDF5 file.
- A package using the extension is not strict WITSML; it is flagged as such by
  the house rules.
- Interoperability with tools that do not know the extension is by refusal, not
  silent misreading.
