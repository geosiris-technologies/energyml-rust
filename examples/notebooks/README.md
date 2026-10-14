# Notebooks

Worked examples of the Python bridge, `energyml-rust` (imported as `energyml.rust`). Each
one answers a question a user of EnergyML packages asks, on real data, and is stored with its
outputs, so that it reads on GitHub without being run.

| Notebook | Question |
|---|---|
| [00 - What is in this package?](00-what-is-in-a-package.ipynb) | open a package, list and find objects, follow references, read an object, its arrays and its geometry |
| [01 - Geometry, from 2D grids to 3D cells](01-geometry-in-3d.ipynb) | evaluate every kind of representation, choose a placement, draw horizons and wells in 3D, read the cells of an IJK grid |
| [02 - Arrays and properties](02-arrays-and-properties.ipynb) | weigh an array, read a slab of it, read the properties of a grid and of a well, with their units |
| [03 - Is this package valid?](03-is-this-package-valid.ipynb) | judge a package: schema, property values, counts, references, the business rules of the standard, and what could not be decided |
| [04 - A package from nothing](04-a-package-from-nothing.ipynb) | build the CRSs, a horizon and a triangulated surface from numpy, write them, read them back and judge them |
| [05 - Editing a package](05-editing-a-package.ipynb) | take objects out with what they need, rename the HDF5 file, change and remove objects, repair what a removal breaks |
| [06 - Exporting to other tools](06-exporting.ipynb) | OBJ, VTK, GeoJSON and CSV, the same metadata in each, and a refusal rather than a gap |

## Running them

```
pip install energyml-rust matplotlib pandas jupyter
jupyter lab
```

from this folder. Every notebook but 04 works on **Lutetia**, a synthetic model (CC BY 4.0) built on
boreholes of the BRGM Banque du sous-sol (Licence Ouverte 2.0), modified for the example,
kept in [geosiris-technologies/energyml-lutetia](https://github.com/geosiris-technologies/energyml-lutetia).
`_lutetia.py` finds it: `$ENERGYML_LUTETIA_DIR` when set (a clone of that repository will
do), otherwise the commit this repository pins, downloaded once into `target/lutetia/` by
`scripts/fetch-lutetia.py`. 04 needs no sample file.

A development build of the bridge is on TestPyPI:

```
pip install --pre -i https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ energyml-rust
```
