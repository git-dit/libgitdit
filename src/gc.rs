// git-dit - the distributed issue tracker for git
// Copyright (C) 2016, 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2016, 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Garbage collecting utilities
//!
//! This module provides git-dit related garbage collection utilites.
//!

use git2::{self, Reference};

use crate::error;
use crate::issue::Issue;
use crate::object;
use crate::reference;
use crate::traversal::{TraversalBuilder, Traversible};
use iter::{self, RefsReferringTo};
use utils::ResultIterExt;

use error::*;
use error::Kind as EK;


/// Reference collecting iterator
///
/// This is a convenience type for a `ReferenceDeletingIter` wrapping an
/// iterator over to-be-collected references.
///
pub type ReferenceCollector<'r> = iter::ReferenceDeletingIter<
    'r,
    <Vec<Reference<'r>> as IntoIterator>::IntoIter
>;


pub enum ReferenceCollectionSpec {
    Never,
    BackedByRemoteHead,
}


/// Type representing collectable references
///
/// Use this type in order to compute dit-references which are no longer
/// required and thus may be collected.
///
pub struct CollectableRefs<'r>
{
    repo: &'r git2::Repository,
    /// Should remote references be considered during collection?
    consider_remote_refs: bool,
    /// Under what circumstances should local heads be collected?
    collect_heads: ReferenceCollectionSpec,
}

impl<'r> CollectableRefs<'r>
{
    /// Create a new CollectableRefs object
    ///
    /// By default only local references are considered, e.g. references which
    /// are unnecessary due to remote references are not reported.
    ///
    pub fn new(repo: &'r git2::Repository) -> Self
    {
        CollectableRefs {
            repo: repo,
            consider_remote_refs: false,
            collect_heads: ReferenceCollectionSpec::Never,
        }
    }

    /// Causes remote references to be considered
    ///
    /// By default, only local references are considered for deciding which
    /// references will be collected. Calling this function causes the resulting
    /// struct to also consider remote references.
    ///
    pub fn consider_remote_refs(mut self, option: bool) -> Self {
        self.consider_remote_refs = option;
        self
    }

    /// Causes local head references to be collected under a specified condition
    ///
    /// By default, heads are never collected. Using this function a user may
    /// change this behaviour.
    ///
    pub fn collect_heads(mut self, condition: ReferenceCollectionSpec) -> Self {
        self.collect_heads = condition;
        self
    }

    /// Find collectable references for an issue
    ///
    /// Construct an iterator yielding all collectable references for a given
    /// issue, according to the configuration.
    pub fn for_issue(
        &self,
        issue: &Issue<'r, git2::Repository>,
    ) -> Result<RefsReferringTo<'r>, git2::Error> {
        use reference::References;

        let mut retval = {
            let messages = self
                .repo
                .revwalk()
                .wrap_with_kind(EK::CannotConstructRevwalk)?;
            RefsReferringTo::new(messages)
        };

        // local head
        if let Some(local_head) = issue.local_head()? {
            // Its ok to ignore failures to retrieve the local head. It will
            // not be present in user's repositories anyway.
            retval.push(
                local_head
                    .peel(git2::ObjectType::Commit)
                    .wrap_with_kind(EK::CannotGetCommit)?
                    .id()
            )?;

            // Whether the local head should be collected or not is computed
            // here, in the exact same way it is for leaves. We do that
            // because can't mix the computation with those of the leaves.
            // It would cause head references to be removed if any message
            // was posted as a reply to the current head.
            let mut head_history = self
                .repo
                .revwalk()
                .wrap_with_kind(EK::CannotConstructRevwalk)?;
            match self.collect_heads {
                ReferenceCollectionSpec::Never => {},
                ReferenceCollectionSpec::BackedByRemoteHead => {
                    for item in issue.all_remote_heads()? {
                        let id = item?
                            .peel(git2::ObjectType::Commit)
                            .wrap_with_kind(EK::CannotGetCommit)?
                            .id();
                        head_history
                            .push(id)
                            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;
                    }
                },
            };
            let mut referring_refs = iter::RefsReferringTo::new(head_history);
            referring_refs.watch_ref(local_head)?;
            referring_refs.collect_result_into(&mut retval)?;
        }

        // local leaves
        for item in issue.local_refs()?.leaves() {
            let leaf = item.wrap_with_kind(error::Kind::CannotGetReference)?;
            // NOTE: We push the parents of the references rather than the
            //       references themselves since that would cause the
            //       `RefsReferringTo` report that exact same reference.
            Self::push_ref_parents(&mut retval, &leaf)?;
            retval.watch_ref(leaf)?;
        }

        // remote refs
        if self.consider_remote_refs {
            for item in issue.all_remote_refs()? {
                let id = item
                    .wrap_with_kind(error::Kind::CannotGetReference)?
                    .peel(git2::ObjectType::Commit)
                    .wrap_with_kind(EK::CannotGetCommit)?
                    .id();
                retval.push(id)?;
            }
        }

        Ok(retval)
    }

    /// Push the parents of a referred commit to a revwalk
    ///
    fn push_ref_parents<'a>(
        target: &mut RefsReferringTo,
        reference: &'a Reference<'a>,
    ) -> Result<(), git2::Error> {
        let referred_commit = reference
            .peel(git2::ObjectType::Commit)
            .wrap_with_kind(EK::CannotGetCommit)?
            .into_commit()
            .map_err(|o| EK::CannotGetCommitForRev(o.id().to_string()))?;
        for parent in referred_commit.parent_ids() {
            target.push(parent)?;
        }
        Ok(())
    }

    /// Retrieve the local reference if it is collectable
    pub fn head<R>(
        &self,
        issue: &Issue<'r, R>,
    ) -> error::Result<Option<R::Reference>, R::InnerError>
    where
        R: reference::Store<'r> + object::Database<'r> + Traversible<'r>,
    {
        use reference::Reference;

        let Some(local_head) = issue.local_head()? else {
            return Ok(None);
        };

        Ok(match self.collect_heads {
            ReferenceCollectionSpec::Never => None,
            ReferenceCollectionSpec::BackedByRemoteHead => {
                let Some(target) = local_head.target() else {
                    return Ok(Some(local_head));
                };

                issue
                    .all_remote_heads()?
                    .try_fold(issue.terminated_messages()?, |i, r| {
                        i.with_heads(r?.target())
                            .map_err(Into::into)
                            .wrap_with_kind(error::Kind::CannotConstructRevwalk)
                    })?
                    .build()
                    .map_err(Into::into)
                    .wrap_with_kind(error::Kind::CannotConstructRevwalk)?
                    .any(|i| i.map(|i| i == target).unwrap_or(false))
                    .then_some(local_head)
            }
        })
    }

    /// Retrieve all collectable leaves for an [Issue]
    pub fn leaves<R>(
        &self,
        issue: &Issue<'r, R>,
    ) -> error::Result<impl Iterator<Item = RefResult<'r, R>>, R::InnerError>
    where
        R: reference::Store<'r> + object::Database<'r> + Traversible<'r>,
    {
        use object::commit::Commit;
        use reference::{Reference, References};

        let mut dead_leaves = Vec::new();

        let mut messages = issue
            .terminated_messages()?
            .with_heads(issue.local_head()?.and_then(|h| h.target()))
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;

        let mut candidates: std::collections::HashMap<_, Vec<_>> = Default::default();

        for reference in issue.local_refs()?.leaves() {
            let reference = reference.wrap_with_kind(error::Kind::CannotGetReference)?;
            if let Some(id) = reference.target() {
                messages = messages
                    .with_heads(issue.repo().find_commit(id.clone())?.parent_ids())
                    .map_err(Into::into)
                    .wrap_with_kind(error::Kind::CannotConstructRevwalk)?;

                candidates.entry(id).or_default().push(Ok(reference));
            } else {
                dead_leaves.push(Ok(reference))
            };
        }

        let collectable = messages
            .build()
            .map_err(Into::into)
            .wrap_with_kind(error::Kind::CannotConstructRevwalk)?
            .map_while(move |i| {
                if candidates.is_empty() {
                    // We can stop looking for references to collect when we ran
                    // out of candidates.
                    None
                } else {
                    Some(match i {
                        Ok(id) => candidates.remove(&id).unwrap_or_default(),
                        Err(e) => vec![Err(e)],
                    })
                }
            });
        Ok(std::iter::once(dead_leaves).chain(collectable).flatten())
    }
}

type RefResult<'r, R> = std::result::Result<
    <R as reference::Store<'r>>::Reference,
    <<R as Traversible<'r>>::TraversalBuilder as TraversalBuilder>::Error,
>;

#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::{TestingRepo, empty_tree};

    use repository::RepositoryExt;

    // CollectableRefs tests

    #[test]
    fn collectable_leaves() {
        let mut testing_repo = TestingRepo::new("collectable_leaves");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);

        let mut refs_to_collect = Vec::new();
        let mut issues = Vec::new();

        {
            // issue not supposed to be affected
            let issue = repo
                .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
                .expect("Could not create issue");
            let initial_message = issue
                .initial_message()
                .expect("Could not retrieve initial message");
            issue.add_message(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message])
                .expect("Could not add message");
        }

        {
            let issue = repo
                .create_issue(&sig, &sig, "Test message 3", &empty_tree, vec![])
                .expect("Could not create issue");
            let initial_message = issue
                .initial_message()
                .expect("Could not retrieve initial message");
            let message = issue
                .add_message(&sig, &sig, "Test message 4", &empty_tree, vec![&initial_message])
                .expect("Could not add message");
            issue.update_head(message.id(), true).expect("Could not update head");
            issues.push(issue);
            refs_to_collect.push(message.id());
        }

        {
            let issue = repo
                .create_issue(&sig, &sig, "Test message 5", &empty_tree, vec![])
                .expect("Could not create issue");
            let initial_message = issue
                .initial_message()
                .expect("Could not retrieve initial message");
            let message1 = issue
                .add_message(&sig, &sig, "Test message 6", &empty_tree, vec![&initial_message])
                .expect("Could not add message");
            issue
                .add_message(&sig, &sig, "Test message 7", &empty_tree, vec![&message1])
                .expect("Could not add message");
            issues.push(issue);
            refs_to_collect.push(message1.id());
        }

        refs_to_collect.sort();

        let collectable = CollectableRefs::new(repo).collect_heads(ReferenceCollectionSpec::BackedByRemoteHead);
        let mut collected: Vec<_> = issues
            .iter()
            .flat_map(|i| collectable.for_issue(i).expect("Error during discovery of collectable refs"))
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("Error during collection")
            .into_iter()
            .map(|r| r.peel(git2::ObjectType::Commit).expect("Could not peel ref").id())
            .collect();
        collected.sort();
        assert_eq!(refs_to_collect, collected);
    }
}

