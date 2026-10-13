# property_kinds.tsv

Compacted from the Energistics `PropertyKindDictionary` v2.3 (eml 2.3), 3 662
kinds, one per line, tab-separated: uuid, title, parent uuid, quantity class,
abstract (0/1), deprecation date, description. This is exactly the format
`dictionary_xml::to_compact` writes and `from_compact` reads; the reader has no
comment syntax, so the provenance lives here and not in the file.

The file is generated (by the private generator, from the Energistics XML),
never edited by hand.

TODO(plan section 2.3): insert here the Energistics notice text required for
redistribution of this material.
