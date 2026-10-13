# How a colour is found

RESQML 2.0.1 has no colour at all; from 2.2 on, colour is carried by graphical
information objects that apply to an object, to a collection of objects, or to a
whole model. An object that carries nothing itself can therefore still be drawn
in a colour that was chosen for something it belongs to. This page states the
order in which the library looks, and what it answers.

The walk lives in `energyml-domain` (`inherit` for the order, which knows nothing
about colours; `color_inherit` for the colour question). It reads relations from
the [package index](adr/0010-package-index-and-caches.md), so no object is opened
to follow a link. The rule that there is one colour decision for every face is in
[ADR 0009](adr/0009-properties-tables-colours.md).

## The order

The first rank that has an answer wins.

| Rank | Where the colour is looked for |
|---|---|
| 1 | on the object itself |
| 2 | on the collections that reference it, then on the collections of those collections |
| 3 | on its parents |
| 4 | on the collections that reference those parents, nested |
| 5 | on the parents of the parents, with the same rules, one stage up at a time |
| 6 | on the model that references the object or one of its parents |

A **parent** is reached through the three fields a representation, an
interpretation and a feature climb by: `RepresentedObject`,
`RepresentedInterpretation` and `InterpretedFeature` (the feature /
interpretation / representation chain, "FIRP"). A **collection** is a membership
written by a `CollectionsToDataobjectsAssociationSet`: a member is in the
collection of the association with the same index.

Within one rank the shortest path wins; at equal length the first in document
order wins, and the other candidates are kept and shown as ties, not hidden.
Rank 5 is read stage by stage, so a collection of a grandparent does not race a
great-grandparent. Some things are noted but not followed: the collections of the
model, and any collection that is a `project`.

A **property** has a shorter path: the property, its property kind, the kind's
parents, a collection at each step, and nothing beyond. A property never takes
the colour of the representation it hangs on, nor of a model.

## What the answer carries

An answer is not just a colour. It carries:

- the **level** it was found at (the object, a collection, a parent, the model),
  read from the last step of the path;
- the **path** from the object to the one that carries the colour: each step says
  where it leaves, where it arrives, by what kind of link (parent, collection,
  model) and through which field;
- the **ties** that lost, and the **notes** about what was skipped and why (a
  cycle, a duplicate association, a colour on a collection that is not followed).

A face that shows a colour can therefore also show why it is that colour.

## Nothing found is no colour

When no rank answers, the answer is **none**. The library does not invent a
default: choosing what to draw then is the decision of the view, which is the one
that knows whether to derive a hue from the identifier, draw grey, or hide the
object. This follows [refuse rather than draw](adr/0005-refuse-rather-than-draw.md).

## Cycles and duplicates

A collection that holds itself, near or far, is a cycle: it is named, and the
walk does not follow the edge that closes it. The same collection named in two
associations of one set counts once, as does an object listed twice in an
association, as the common schema's rules on collections require; both cases are
counted in the notes.
