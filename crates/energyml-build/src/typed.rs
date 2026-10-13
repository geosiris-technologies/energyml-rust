//! What an operation's author writes — **typed** — and the adapter that puts
//! it behind the interface the registry, the WebAssembly front end and the CLI run.
//!
//! Two traits.
//! [`Operation`] is written against a struct of parameters and a struct of
//! inputs, both derived ([`crate::declare`]); `plan` receives them already
//! read and bound, and never looks a name up. [`AnyOperation`] is the erased
//! interface everything else executes — the one every operation implemented
//! directly before this, and the one an operation not yet migrated still
//! implements. [`typed`] turns the first into the second.
//!
//! # What the adapter does, in order
//!
//! - `takes` is the inputs' slots, `needs` the parameters' slots with the
//!   operation's suggestions posed on them — a suggestion that names no slot
//!   is a **failed assertion** in a debug build, never a silent loss;
//! - `fitness` binds the inputs (the kind of each piece, its count), then asks
//!   the operation's own [`Operation::check`];
//! - `plan` binds, checks, reads the settled choices into the parameters, and
//!   calls the operation with a [`Context`].
//!
//! [`AnyOperation`]: crate::operation::AnyOperation

use crate::declare::{Inputs, Params, Suggestions};
use crate::operation::{
    AnyOperation, Category, Choices, Error, Fitness, Identity, Output, Plan, Purpose, Refusal, Slot, When,
};
use crate::pieces::Pieces;

/// Who an operation is — what a listing, a form title and an activity read.
#[derive(Debug, Clone, Copy)]
pub struct About {
    /// `thickness`, `interpolate` — the name a caller asks for.
    pub name: &'static str,
    /// One line, for `--list` and a form.
    pub describe: &'static str,
    /// Where a listing ranges it.
    pub category: Category,
    /// The qualified types it makes.
    pub makes: &'static [&'static str],
    /// Its key in `rc/activityTemplates.json`.
    pub recipe: &'static str,
    /// The feature and interpretation it writes when none is named.
    pub represents: Option<(&'static str, &'static str)>,
}

/// How a long computation says where it is, and learns it should stop.
///
/// QGIS passes one of these to every algorithm (`QgsProcessingFeedback`), and
/// the front end needs the same: the session runs in a worker, and a long
/// computation made the whole session wait with no word of progress and no way
/// to stop it but killing the worker. Silent by default.
pub trait Feedback: Sync {
    /// The fraction done, in `[0, 1]`.
    fn progress(&self, _done: f64) {}
    /// Whether the caller asked to stop.
    fn cancelled(&self) -> bool {
        false
    }
}

/// The feedback nobody listens to.
pub struct Silent;

impl Feedback for Silent {}

/// What a plan is computed with, beside its inputs and parameters.
pub struct Context<'a> {
    /// How the objects made are named.
    pub identity: &'a Identity,
    /// When, and by whom.
    pub when: &'a When,
    /// The geometry alone, or the writing.
    pub purpose: Purpose,
    /// Where progress goes, and where a stop comes from.
    pub feedback: &'a dyn Feedback,
    /// The bag as the caller gave it. An operation reads its **inputs** from
    /// what was bound, never from here; what it may read here is what the
    /// caller **resolved** — [`Context::known`] — and the seed of the run —
    /// [`Context::seed`]. And, until the last writer is typed, a derived bag
    /// handed to one that still reads pieces by name.
    pub bag: &'a Pieces,
}

impl<'a> Context<'a> {
    /// Everything the caller resolved about the objects involved — the
    /// inputs' **and the answers'**: a CRS picked in a form comes back as a
    /// piece of its own, carrying the vertical half of its frame, which is not
    /// an input and is what a writer needs to reference it.
    pub fn known(&self) -> Vec<&'a crate::pieces::Known> {
        self.bag.objects().map(|(_, known)| known).collect()
    }

    /// The seed the run's derived identities hang from — the first piece the
    /// caller gave, and the title. What `support::seed_of` has always been.
    pub fn seed(&self, title: &str) -> String {
        crate::ops::support::seed_of(self.bag, title)
    }

    /// The name of the first piece the caller gave — what a derived name
    /// (`outline of …`) is built on, so that it names what it always named.
    pub fn first(&self) -> String {
        self.bag
            .iter()
            .next()
            .map(|(name, _)| name.to_string())
            .unwrap_or_default()
    }

    /// A refusal if the caller asked to stop — for a long loop to call.
    pub fn stop_if_cancelled(&self) -> Result<(), Refusal> {
        if self.feedback.cancelled() {
            Err(Refusal::new(
                "cancelled",
                "the computation was stopped before it finished",
            ))
        } else {
            Ok(())
        }
    }
}

/// A way of creating objects, written against typed parameters and inputs.
pub trait Operation: Send + Sync + 'static {
    /// The parameters — a derived struct, or `()`.
    type Params: Params;
    /// The inputs — a derived struct, or `()`.
    type Inputs: Inputs;

    /// Who it is.
    fn about(&self) -> About;

    /// **The other categories it is also listed under** — none by default.
    ///
    /// Some operations belong in several categories — for example the one that
    /// extracts wellhead points to create a point set is filed under "write" but
    /// could also be under "Wells". Declared here
    /// rather than guessed by the front end from what the operation takes: a slot of
    /// shapes carries no type to guess from, and the module is where an
    /// operation says what it is.
    fn also(&self) -> &'static [Category] {
        &[]
    }

    /// Propose values read off the inputs — the CRS of the surface, the
    /// direction of its vertical axis. Runs on **any** bag, the empty one
    /// included; it poses values on declared slots, by name.
    fn suggest(&self, _bag: &Pieces, _slots: &mut Suggestions<'_>) {}

    /// What the kind and count of the inputs cannot say — a base with no face.
    fn check(&self, _inputs: &Self::Inputs) -> Result<(), Refusal> {
        Ok(())
    }

    /// The plan — pure, from what was bound and read.
    fn plan(&self, inputs: Self::Inputs, params: Self::Params, cx: &Context<'_>) -> Result<Plan, Refusal>;

    /// What the activity records that is neither a choice nor an output.
    fn reports(&self) -> Vec<Slot> {
        Vec::new()
    }

    /// What it writes, by role — [`AnyOperation::gives`]. Empty: not declared yet.
    fn gives(&self) -> Vec<Output> {
        Vec::new()
    }
}

/// A typed operation, behind the erased interface.
pub struct Typed<T>(pub T);

/// Put a typed operation in a registry.
pub fn typed<T: Operation>(operation: T) -> Box<dyn AnyOperation> {
    Box::new(Typed(operation))
}

impl<T: Operation> Typed<T> {
    fn bound(&self, pieces: &Pieces) -> Result<T::Inputs, Refusal> {
        let inputs = T::Inputs::bind(pieces)?;
        self.0.check(&inputs)?;
        Ok(inputs)
    }
}

impl<T: Operation> AnyOperation for Typed<T> {
    fn name(&self) -> &'static str {
        self.0.about().name
    }

    fn describe(&self) -> &'static str {
        self.0.about().describe
    }

    fn makes(&self) -> &'static [&'static str] {
        self.0.about().makes
    }

    fn recipe(&self) -> &'static str {
        self.0.about().recipe
    }

    fn category(&self) -> Category {
        self.0.about().category
    }

    fn also(&self) -> &'static [Category] {
        self.0.also()
    }

    fn represents(&self) -> Option<(&'static str, &'static str)> {
        self.0.about().represents
    }

    fn fitness(&self, pieces: &Pieces) -> Fitness {
        match self.bound(pieces) {
            Ok(_) => Fitness::Fit,
            Err(refusal) => Fitness::Unfit(Refusal {
                key: "unfit",
                ..refusal
            }),
        }
    }

    fn takes(&self) -> Vec<Slot> {
        T::Inputs::slots()
    }

    fn needs(&self, pieces: &Pieces) -> Vec<Slot> {
        let mut slots = T::Params::slots();
        let mut suggestions = Suggestions::new(&mut slots);
        self.0.suggest(pieces, &mut suggestions);
        debug_assert!(
            suggestions.missed().is_empty(),
            "`{}` suggests for slots it does not declare: {:?}",
            self.name(),
            suggestions.missed()
        );
        slots
    }

    fn reports(&self) -> Vec<Slot> {
        self.0.reports()
    }

    fn gives(&self) -> Vec<Output> {
        self.0.gives()
    }

    fn plan(
        &self,
        pieces: &Pieces,
        choices: &Choices,
        identity: &Identity,
        when: &When,
    ) -> Result<Plan, Error> {
        let cx = Context {
            identity,
            when,
            purpose: Purpose::Writing,
            feedback: &Silent,
            bag: pieces,
        };
        self.plan_with(pieces, choices, &cx)
    }

    fn plan_with(&self, pieces: &Pieces, choices: &Choices, cx: &Context<'_>) -> Result<Plan, Error> {
        let inputs = self.bound(pieces)?;
        let params = T::Params::read(choices)?;
        Ok(self.0.plan(inputs, params, cx)?)
    }
}
