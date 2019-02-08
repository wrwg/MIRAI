// Copyright (c) Facebook, Inc. and its affiliates.
//
// This source code is licensed under the MIT license found in the
// LICENSE file in the root directory of this source tree.

use abstract_domains::{AbstractDomains, ExpressionDomain};
use abstract_value::{self, AbstractValue, Path};
use rpds::HashTrieMap;
use rustc::mir::BasicBlock;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result};

#[derive(Clone, Eq, PartialEq)]
pub struct Environment {
    /// The disjunction of all the exit conditions from the predecessors of this block.
    pub entry_condition: AbstractValue,
    /// The conditions that guard exit from this block to successor blocks
    pub exit_conditions: HashMap<BasicBlock, AbstractValue>,
    /// Does not include any entries where the value is abstract_value::Bottom
    pub value_map: HashTrieMap<Path, AbstractValue>,
}

/// Default
impl Environment {
    pub fn default() -> Environment {
        Environment {
            entry_condition: abstract_value::TRUE,
            exit_conditions: HashMap::default(),
            value_map: HashTrieMap::default(),
        }
    }
}

impl Debug for Environment {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.value_map.fmt(f)
    }
}

/// Constructor
impl Environment {
    /// Returns an environment that has a path for every parameter of the given function,
    /// initialized with abstract_values::Top
    pub fn with_parameters(num_args: usize) -> Environment {
        let mut value_map = HashTrieMap::default();
        for i in 1..=num_args {
            let par_i = Path::LocalVariable { ordinal: i };
            value_map = value_map.insert(par_i, abstract_value::TOP);
        }
        Environment {
            value_map,
            entry_condition: abstract_value::TRUE,
            exit_conditions: HashMap::default(),
        }
    }
}

/// Methods
impl Environment {
    /// Returns a reference to the value associated with the given path, if there is one.
    pub fn value_at(&self, path: &Path) -> Option<&AbstractValue> {
        self.value_map.get(path)
    }

    /// Updates the path to value map so that the given path now points to the given value.
    pub fn update_value_at(&mut self, path: Path, value: AbstractValue) {
        debug!("updating value of {:?} to {:?}", path, value);
        if let Some((join_condition, true_path, false_path)) = self.try_to_split(&path) {
            // If path is an abstraction that can match more than one path, we need to do weak updates.
            let top = abstract_value::TOP;
            let true_val = value.join(self.value_at(&true_path).unwrap_or(&top), &join_condition);;
            let false_val = self
                .value_at(&false_path)
                .unwrap_or(&top)
                .join(&value, &join_condition);
            self.update_value_at(true_path, true_val);
            self.update_value_at(false_path, false_val);
        }
        if value.is_bottom() {
            self.value_map = self.value_map.remove(&path);
        } else {
            self.value_map = self.value_map.insert(path, value);
        }
    }

    /// If the path contains an abstract value that was constructed with a join, the path is
    /// concretized into two paths where the abstract value is replaced by the consequent
    /// and alternate, respectively. These paths can then be weakly updated to reflect the
    /// lack of precise knowledge at compile time.
    fn try_to_split(&mut self, path: &Path) -> Option<(AbstractValue, Path, Path)> {
        match path {
            Path::LocalVariable { .. } => {
                let val_opt = self.value_at(path);
                if let Some(val) = val_opt {
                    if let AbstractValue {
                        value:
                            AbstractDomains {
                                expression_domain:
                                    ExpressionDomain::ConditionalExpression {
                                        condition,
                                        consequent,
                                        alternate,
                                    },
                            },
                        provenance,
                    } = val
                    {
                        match ((**consequent).clone(), (**alternate).clone()) {
                            (
                                ExpressionDomain::AbstractHeapAddress(addr1),
                                ExpressionDomain::AbstractHeapAddress(addr2),
                            ) => {
                                return Some((
                                    AbstractValue {
                                        provenance: provenance.clone(),
                                        value: (**condition).clone(),
                                    },
                                    Path::AbstractHeapAddress { ordinal: addr1 },
                                    Path::AbstractHeapAddress { ordinal: addr2 },
                                ));
                            }
                            (
                                ExpressionDomain::Reference(path1),
                                ExpressionDomain::Reference(path2),
                            ) => {
                                return Some((
                                    AbstractValue {
                                        provenance: provenance.clone(),
                                        value: (**condition).clone(),
                                    },
                                    path1,
                                    path2,
                                ));
                            }
                            _ => (),
                        }
                    }
                };
                None
            }
            Path::QualifiedPath {
                ref qualifier,
                ref selector,
            } => {
                if let Some((join_condition, true_path, false_path)) = self.try_to_split(qualifier)
                {
                    let true_path = Path::QualifiedPath {
                        qualifier: box true_path,
                        selector: selector.clone(),
                    };
                    let false_path = Path::QualifiedPath {
                        qualifier: box false_path,
                        selector: selector.clone(),
                    };
                    return Some((join_condition, true_path, false_path));
                }
                None
            }
            _ => None,
        }
    }

    /// Returns an environment with a path for every entry in self and other and an associated
    /// value that is the join of self.value_at(path) and other.value_at(path)
    pub fn join(&self, other: &Environment, join_condition: &AbstractValue) -> Environment {
        self.join_or_widen(other, join_condition, |x, y, c| x.join(y, c))
    }

    /// Returns an environment with a path for every entry in self and other and an associated
    /// value that is the widen of self.value_at(path) and other.value_at(path)
    pub fn widen(&self, other: &Environment, join_condition: &AbstractValue) -> Environment {
        self.join_or_widen(other, join_condition, |x, y, c| x.widen(y, c))
    }

    /// Returns an environment with a path for every entry in self and other and an associated
    /// value that is the join or widen of self.value_at(path) and other.value_at(path).
    fn join_or_widen(
        &self,
        other: &Environment,
        join_condition: &AbstractValue,
        join_or_widen: fn(&AbstractValue, &AbstractValue, &AbstractValue) -> AbstractValue,
    ) -> Environment {
        let value_map1 = &self.value_map;
        let value_map2 = &other.value_map;
        let mut value_map: HashTrieMap<Path, AbstractValue> = HashTrieMap::default();
        for (path, val1) in value_map1.iter() {
            let p = path.clone();
            match value_map2.get(path) {
                Some(val2) => {
                    value_map = value_map.insert(p, join_or_widen(&val1, &val2, &join_condition));
                }
                None => {
                    debug_assert!(!val1.is_bottom());
                    value_map = value_map.insert(
                        p,
                        join_or_widen(&val1, &abstract_value::BOTTOM, &join_condition),
                    );
                }
            }
        }
        for (path, val2) in value_map2.iter() {
            if !value_map1.contains_key(path) {
                debug_assert!(!val2.is_bottom());
                let p = path.clone();
                value_map = value_map.insert(
                    p,
                    join_or_widen(&abstract_value::BOTTOM, &val2, &join_condition),
                );
            }
        }
        Environment {
            value_map,
            entry_condition: abstract_value::TRUE,
            exit_conditions: HashMap::default(),
        }
    }

    /// Returns true if for every path, self.value_at(path).subset(other.value_at(path))
    pub fn subset(&self, other: &Environment) -> bool {
        let value_map1 = &self.value_map;
        let value_map2 = &other.value_map;
        if value_map1.size() > value_map2.size() {
            // There is a key in value_map1 that has a value that is not bottom and which is not
            // present in value_map2 (and therefore is bottom), hence there is a path where
            // !(self[path] <= other[path])
            return false;
        }
        for (path, val1) in value_map1.iter() {
            match value_map2.get(path) {
                Some(val2) => {
                    if !(val1.subset(val2)) {
                        return false;
                    }
                }
                None => {
                    debug_assert!(!val1.is_bottom());
                    return false;
                }
            }
        }
        true
    }
}