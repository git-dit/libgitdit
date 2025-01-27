// git-dit - the distributed issue tracker for git
// Copyright (C) 2017 Matthias Beyer <mail@beyermatthias.de>
// Copyright (C) 2017 Julian Ganz <neither@nut.email>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//

//! Module providing extension trait for remotes

use std::str::Utf8Error;

use git2::Remote;

use crate::base::Base;
use issue::Issue;

/// Container for remote names
pub trait Names {
    /// An [Iterator] over remote names
    type NameIter<'n>: Iterator<Item: Name>
    where
        Self: 'n;

    /// Get an [Iterator] over all remotes' names
    fn names(&self) -> Self::NameIter<'_>;
}

impl Names for git2::string_array::StringArray {
    type NameIter<'n> = git2::string_array::IterBytes<'n>;

    fn names(&self) -> Self::NameIter<'_> {
        self.iter_bytes()
    }
}

impl Names for Vec<String> {
    type NameIter<'n>
        = std::iter::Map<std::slice::Iter<'n, String>, fn(&String) -> &[u8]>
    where
        Self: 'n;

    fn names(&self) -> Self::NameIter<'_> {
        self.iter().map(AsRef::as_ref)
    }
}

/// Name of a remote git repository
pub trait Name {
    /// Reference prefix for this repository
    ///
    /// This fn will return the reference prefix of this remote in the form of a
    /// path, like `refs/remotes/<remote-name>`. Its default implementation
    /// returns any error [as_str](Self::as_str) returns.
    fn ref_path(&self) -> Result<String, Utf8Error> {
        self.as_str().map(|s| format!("{REMOTES_REF_BASE}/{s}"))
    }

    /// Represenation of this name as a `&str`
    ///
    /// If this name can be represented as a `&str` without loss of information,
    /// this fn will return that representation.
    fn as_str(&self) -> Result<&str, Utf8Error>;
}

impl Name for &[u8] {
    fn as_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self)
    }
}

/// Extension trait for remotes
///
pub trait RemoteExt {
    /// Get the refspec for a specific issue for this remote
    ///
    /// A refspec will only be returned if the remote has a (valid) name.
    fn issue_refspec(&self, issue: Issue<'_, impl Base>) -> Option<String>;

    /// Get the refspec for all issue for this remote
    ///
    /// A refspec will only be returned if the remote has a (valid) name.
    ///
    fn all_issues_refspec(&self) -> Option<String>;
}

impl<'r> RemoteExt for Remote<'r> {
    fn issue_refspec(&self, issue: Issue<'_, impl Base>) -> Option<String> {
        self.name()
            .map(|n| format!("+refs/dit/{0}/*:refs/remotes/{n}/dit/{0}/*", issue.id()))
    }

    fn all_issues_refspec(&self) -> Option<String> {
        self.name()
            .map(|name| format!("+refs/dit/*:refs/remotes/{0}/dit/*", name))
    }
}

const REMOTES_REF_BASE: &str = "refs/remotes";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_as_str() {
        assert_eq!(b"foo".as_slice().as_str(), Ok("foo"));
    }

    #[test]
    fn name_ref_path() {
        assert_eq!(
            b"foo".as_slice().ref_path(),
            Ok("refs/remotes/foo".to_owned()),
        );
    }
}
