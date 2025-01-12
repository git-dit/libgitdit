// git-dit - the distributed issue tracker for git
// Copyright (C) 2016, 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2016, 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Repository related utilities
//!
//! This module provides the `RepositoryExt` extension trait which provides
//! issue handling utilities for repositories.
//!

use std::collections::HashSet;

use git2::{self, Commit, Tree};

use crate::base::Base;
use crate::error;
use crate::reference;
use crate::traversal::Traversible;
use gc;
use issue::Issue;
use iter;
use utils::ResultIterExt;

use error::*;
use error::{Kind as EK};


/// Set of unique issues
pub type UniqueIssues<'r, R> = HashSet<Issue<'r, R>>;


/// Extension trait for Repositories
///
/// This trait is intended as an extension for repositories. It introduces
/// utility functions for dealing with issues, e.g. for retrieving references
/// for issues, creating messages and finding the initial message of an issue.
pub trait RepositoryExt<'r>: Base + Sized {
    /// Retrieve an issue
    ///
    /// Returns the issue with a given id.
    fn find_issue(&'r self, id: Self::Oid) -> Result<Issue<'r, Self>, Self::InnerError>;

    /// Retrieve an issue by its head ref
    ///
    /// Returns the issue associated with a head reference.
    fn issue_by_head_ref(
        &'r self,
        head_ref: &Self::Reference<'_>,
    ) -> Result<Issue<'r, Self>, Self::InnerError>;

    /// Find the issue with a given message in it
    ///
    /// Returns the issue containing the message provided
    fn issue_with_message(&'r self, message: Self::Oid) -> Result<Issue<'r, Self>, Self::InnerError>
    where
        Self: Traversible<'r>,
    {
        for message in self.first_parent_messages(message.clone())? {
            let message = message.map_err(Into::into).wrap_with_kind(EK::Other)?;
            if let Ok(issue) = self.find_issue(message) {
                return Ok(issue);
            }
        }

        Err(EK::NoTreeInitFound(message).into())
    }

    /// Get issue hashes for a prefix
    ///
    /// This function returns all known issues known to the DIT repo under the
    /// prefix provided (e.g. all issues for which refs exist under
    /// `<prefix>/dit/`). Provide "refs" as the prefix to get only local issues.
    fn issues_with_prefix(
        &'r self,
        prefix: &str,
    ) -> Result<UniqueIssues<'r, Self>, Self::InnerError>;

    /// Get all issue hashes
    ///
    /// This function returns all known issues known to the DIT repo.
    fn issues(&'r self) -> Result<UniqueIssues<'r, Self>, Self::InnerError>;

    /// Create a new issue with an initial message
    fn create_issue<'a, A, I, J>(
        &'r self,
        author: &git2::Signature,
        committer: &git2::Signature,
        message: A,
        tree: &Tree,
        parents: I,
    ) -> Result<Issue<'r, Self>, Self::InnerError>
    where
        A: AsRef<str>,
        I: IntoIterator<Item = &'a Commit<'a>, IntoIter = J>,
        J: Iterator<Item = &'a Commit<'a>>;

    /// Produce a CollectableRefs
    fn collectable_refs(&'r self) -> gc::CollectableRefs<'r>;
}

impl<'r> RepositoryExt<'r> for git2::Repository {
    fn find_issue(&'r self, id: Self::Oid) -> Result<Issue<'r, Self>, Self::InnerError> {
        let retval = Issue::new_unchecked(self, id);

        // make sure the id refers to an issue by checking whether an associated
        // head reference exists
        if retval.heads()?.next().is_some() {
            Ok(retval)
        } else {
            Err(EK::CannotFindIssueHead(id).into())
        }
    }

    fn issue_by_head_ref(
        &'r self,
        head_ref: &Self::Reference<'_>,
    ) -> Result<Issue<'r, Self>, Self::InnerError> {
        use reference::Reference;

        head_ref
            .parts()
            .filter(|p| p.kind == reference::Kind::Head)
            .map(|p| Issue::new_unchecked(self, p.issue))
            .ok_or_else(|| {
                error::Kind::MalFormedHeadReference(Reference::name(head_ref).into()).into()
            })
    }

    fn issues_with_prefix(
        &'r self,
        prefix: &str,
    ) -> Result<UniqueIssues<'r, Self>, Self::InnerError> {
        let glob = format!("{}/dit/**/head", prefix);
        self.references_glob(&glob)
            .wrap_with_kind(EK::CannotGetReferences(glob))
            .map(|refs| iter::HeadRefsToIssuesIter::new(self, refs))?
            .collect_result()
    }

    fn issues(&'r self) -> Result<UniqueIssues<'r, Self>, Self::InnerError> {
        let glob = "**/dit/**/head";
        self.references_glob(glob)
            .wrap_with(|| EK::CannotGetReferences(glob.to_owned()))
            .map(|refs| iter::HeadRefsToIssuesIter::new(self, refs))?
            .collect_result()
    }

    fn create_issue<'a, A, I, J>(
        &'r self,
        author: &git2::Signature,
        committer: &git2::Signature,
        message: A,
        tree: &Tree,
        parents: I,
    ) -> Result<Issue<'r, Self>, Self::InnerError>
    where
        A: AsRef<str>,
        I: IntoIterator<Item = &'a Commit<'a>, IntoIter = J>,
        J: Iterator<Item = &'a Commit<'a>>,
    {
        let parent_vec : Vec<&Commit> = parents.into_iter().collect();

        self.commit(None, author, committer, message.as_ref(), tree, &parent_vec)
            .wrap_with_kind(EK::CannotCreateMessage)
            .and_then(|id| {
                let issue = Issue::new_unchecked(self, id);
                issue.update_head(issue.id(), true)?;
                Ok(issue)
            })
    }

    fn collectable_refs(&'r self) -> gc::CollectableRefs<'r> {
        gc::CollectableRefs::new(self)
    }
}




#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::{TestingRepo, empty_tree};

    // RepositoryExt tests

    #[test]
    fn find_issue() {
        let mut testing_repo = TestingRepo::new("find_issue");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree(repo), vec![])
            .expect("Could not create issue");

        repo.find_issue(issue.id())
            .expect("Could not tretrieve issue by id");
    }

    #[test]
    fn issue_by_head_ref() {
        let mut testing_repo = TestingRepo::new("issue_by_head_ref");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree(repo), vec![])
            .expect("Could not create issue");

        let local_head = issue
            .local_head()
            .expect("Could not retrieve local head reference");
        let retrieved_issue = repo
            .issue_by_head_ref(&local_head)
            .expect("Could not retrieve issue");
        assert_eq!(issue.id(), retrieved_issue.id());
    }

    #[test]
    fn issue_with_message() {
        let mut testing_repo = TestingRepo::new("issue_with_message");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let retrieved_issue = repo
            .issue_with_message(message.id())
            .expect("Could not retrieve issue");
        assert_eq!(retrieved_issue.id(), issue.id());
    }

    #[test]
    fn issues() {
        let mut testing_repo = TestingRepo::new("issues");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree(repo), vec![])
            .expect("Could not create issue");

        let mut issues = repo
            .issues()
            .expect("Could not retrieve issues")
            .into_iter();
        let retrieved_issue = issues
            .next()
            .expect("Could not retrieve issue");
        assert_eq!(retrieved_issue.id(), issue.id());
        assert!(issues.next().is_none());
    }

    #[test]
    fn first_parent_messages() {
        let mut testing_repo = TestingRepo::new("first_parent_revwalk");
        let repo = testing_repo.repo();

        let sig = git2::Signature::now("Foo Bar", "foo.bar@example.com")
            .expect("Could not create signature");
        let empty_tree = empty_tree(repo);
        let issue = repo
            .create_issue(&sig, &sig, "Test message 1", &empty_tree, vec![])
            .expect("Could not create issue");
        let initial_message = issue
            .initial_message()
            .expect("Could not retrieve initial message");
        let message = issue
            .add_message(&sig, &sig, "Test message 2", &empty_tree, vec![&initial_message])
            .expect("Could not add message");

        let mut iter = repo
            .first_parent_messages(message.id())
            .expect("Could not create first parent iterator");
        assert_eq!(iter.next().unwrap().unwrap(), message.id());
        assert_eq!(iter.next().unwrap().unwrap(), issue.id());
        assert!(iter.next().is_none());
    }
}

