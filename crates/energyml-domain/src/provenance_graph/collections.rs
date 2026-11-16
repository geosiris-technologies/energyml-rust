//! `PackageIndex::expand_collection`: the members of a `DataobjectCollection`.
//!
//! An inherent `impl` may live in any module of the crate that defines the type;
//! this one uses **public** methods of the index only, so `package_index.rs` is
//! not touched.

use std::collections::HashSet;

use crate::package_index::PackageIndex;

/// Why a collection's members are not listed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandRefusal {
    /// The address names no node of the index.
    #[error("{address} is no node of the index")]
    NotInIndex {
        /// The address asked for.
        address: String,
    },
    /// The node is not a `DataobjectCollection`.
    #[error("{uri} is a {qualified_type}, not a DataobjectCollection")]
    NotACollection {
        /// The node's uri.
        uri: String,
        /// Its qualified type.
        qualified_type: String,
    },
    /// No association set names the collection: it does not have zero members,
    /// it declares none.
    #[error("no CollectionsToDataobjectsAssociationSet names {uri}: its members are not stated anywhere")]
    NoAssociationSet {
        /// The collection's uri.
        uri: String,
    },
    /// More members than the caller allows.
    #[error("{uri} holds {members} objects, more than the {limit} asked for")]
    TooMany {
        /// The collection's uri.
        uri: String,
        /// How many members it has.
        members: usize,
        /// The limit asked for.
        limit: usize,
    },
}

impl PackageIndex {
    /// The members of a `DataobjectCollection`, from the association sets that
    /// name it (common TRG 2.3 §8.3), deduplicated in set then association
    /// order. Full uris.
    ///
    /// Refuses by name: an address that is no node, a node that is not a
    /// collection, a collection no set names (**not** an empty list: a
    /// collection without a set states no members at all), and more members than
    /// `limit`.
    pub fn expand_collection(&self, collection: &str, limit: usize) -> Result<Vec<String>, ExpandRefusal> {
        let node = self.node(collection).ok_or_else(|| ExpandRefusal::NotInIndex {
            address: collection.to_string(),
        })?;
        if &*node.type_key != "DataobjectCollection" {
            return Err(ExpandRefusal::NotACollection {
                uri: node.uri.to_string(),
                qualified_type: node.qualified_type.to_string(),
            });
        }
        let named_by = self.collection_named_by(&node.uri);
        if named_by.is_empty() {
            return Err(ExpandRefusal::NoAssociationSet {
                uri: node.uri.to_string(),
            });
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut members: Vec<String> = Vec::new();
        for (set, index) in named_by {
            for member in self.members_of_association(&set.uri, index) {
                let uri = member.uri.to_string();
                if seen.insert(uri.clone()) {
                    members.push(uri);
                }
            }
        }
        if members.len() > limit {
            return Err(ExpandRefusal::TooMany {
                uri: node.uri.to_string(),
                members: members.len(),
                limit,
            });
        }
        Ok(members)
    }
}
