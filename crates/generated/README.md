# Generated crates

The crates in this directory are **generated code**. They are produced by an internal Geosiris generator from the Energistics XML schemas (and, for ETP, the Avro schemas), and written here by a regeneration bot.

**Do not edit them by hand.** A CI check rejects any pull request that touches `crates/generated/**` unless it comes from the bot. If you find a defect — a missing field, a wrong type, a name that does not match the schema — **open an issue** that names the schema type and version. Do not send a pull request: the fix belongs in the generator, and the next regeneration would overwrite a hand edit.

Provenance: each source file begins with the Energistics notice of its source and the generator version. `schemas.lock` (TODO(verify): to come) will record the SHA-256 of every input schema; it contains no Energistics content. See [NOTICE](../../NOTICE) at the repository root for the attribution and the Energistics Product License Agreement that apply to these crates.

## The twelve crates

| Crate | Standard | Version |
|---|---|---|
| `energyml-eml-2-0` | Energistics common (EML) | 2.0 |
| `energyml-eml-2-1` | Energistics common (EML) | 2.1 |
| `energyml-eml-2-2` | Energistics common (EML) | 2.2 |
| `energyml-eml-2-3` | Energistics common (EML) | 2.3 |
| `energyml-resqml-2-0-1` | RESQML | 2.0.1 |
| `energyml-resqml-2-2-dev3` | RESQML | 2.2 (dev3 pre-release) |
| `energyml-resqml-2-2` | RESQML | 2.2 |
| `energyml-witsml-2-0` | WITSML | 2.0 |
| `energyml-witsml-2-1` | WITSML | 2.1 |
| `energyml-prodml-2-0` | PRODML | 2.0 |
| `energyml-prodml-2-2` | PRODML | 2.2 |
| `energyml-prodml-2-3` | PRODML | 2.3 |

The ETP types (`retp-types`, generated from the ETP v1.2 Avro schemas) belong to the ETP repository, not to this one.

TODO(verify): the use of the names RESQML, WITSML and PRODML in crate names is pending an answer from The Open Group / Energistics.

Energistics®, WITSML™, PRODML™ and RESQML™ are trademarks or registered trademarks of Energistics Consortium, Inc. This work is not affiliated with or endorsed by Energistics or The Open Group.
