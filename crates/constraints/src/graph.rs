//! Variable/rule graph, connected components and backend classification.

use std::collections::BTreeMap;

use crate::{Backend, Problem, VarId};

/// A connected component of free variables and the rules that couple them.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// Free variables (sorted).
    pub vars: Vec<VarId>,
    /// Indices into `Problem::rules` (in problem order).
    pub rules: Vec<usize>,
    /// Backend selected for the whole component.
    pub backend: Backend,
}

struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }

    fn find(&mut self, mut a: usize) -> usize {
        while let Some(&p) = self.parent.get(a) {
            if p == a {
                break;
            }
            let gp = self.parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.parent.get_mut(a) {
                *slot = gp;
            }
            a = p;
        }
        a
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Smaller root wins for deterministic representatives.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            if let Some(slot) = self.parent.get_mut(hi) {
                *slot = lo;
            }
        }
    }
}

/// Split a problem into independent components.
///
/// * Fixed variables never connect rules (they are constants).
/// * Rules with no free variable form their own [`Backend::Trivial`] component
///   (checked for violation only).
/// * Free variables touched only by targets/stays form trivial components.
/// * A component is [`Backend::Linear`] when every row of every rule is affine in
///   the free variables; otherwise the whole component is [`Backend::Numeric`].
#[must_use]
pub fn components(p: &Problem) -> Vec<Component> {
    let n = p.vars.len();
    let mut dsu = Dsu::new(n);
    let fixed = |v: VarId| p.is_fixed(v);
    let mut rule_vars: Vec<Vec<VarId>> = Vec::with_capacity(p.rules.len());
    for r in &p.rules {
        let vs: Vec<VarId> = r.vars().into_iter().filter(|v| !fixed(*v)).collect();
        for w in vs.windows(2) {
            dsu.union(w[0].index(), w[1].index());
        }
        rule_vars.push(vs);
    }
    let mut by_root: BTreeMap<usize, Component> = BTreeMap::new();
    let mut constant_rules = Vec::new();
    for (ri, vs) in rule_vars.iter().enumerate() {
        match vs.first() {
            None => constant_rules.push(ri),
            Some(v) => {
                let root = dsu.find(v.index());
                by_root
                    .entry(root)
                    .or_insert_with(|| Component { vars: Vec::new(), rules: Vec::new(), backend: Backend::Linear })
                    .rules
                    .push(ri);
            }
        }
    }
    let x = p.values();
    for v in 0..n {
        let id = VarId(u32::try_from(v).unwrap_or(u32::MAX));
        if fixed(id) {
            continue;
        }
        let root = dsu.find(v);
        by_root
            .entry(root)
            .or_insert_with(|| Component { vars: Vec::new(), rules: Vec::new(), backend: Backend::Trivial })
            .vars
            .push(id);
    }
    let mut out: Vec<Component> = by_root.into_values().collect();
    for c in &mut out {
        if c.rules.is_empty() {
            c.backend = Backend::Trivial;
            continue;
        }
        let linear = c.rules.iter().all(|ri| {
            p.rules.get(*ri).is_some_and(|r| {
                r.unsupported.is_none() && r.rows.iter().all(|row| row.expr.linear_form(&x, &fixed).is_some())
            })
        });
        c.backend = if linear { Backend::Linear } else { Backend::Numeric };
    }
    for ri in constant_rules {
        out.push(Component { vars: Vec::new(), rules: vec![ri], backend: Backend::Trivial });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Expr, Rule, Strength, Variable, rules};

    #[test]
    fn components_split_and_classify() {
        let mut p = Problem::default();
        let a = p.add_var(Variable::new(1.0));
        let b = p.add_var(Variable::new(2.0));
        let c = p.add_var(Variable::new(3.0));
        let d = p.add_var(Variable::new(4.0));
        let f = p.add_var(Variable::new(5.0).fixed(true));
        // a,b linear; c,d nonlinear; f joins nothing.
        p.rules.push(Rule::new(1, rules::equal(Expr::Var(a), Expr::Var(b), 1.0), Strength::Required));
        p.rules.push(Rule::new(2, rules::equal(Expr::Var(a), Expr::Var(f), 1.0), Strength::Required));
        p.rules.push(Rule::new(3, rules::fix(Expr::mul(Expr::Var(c), Expr::Var(d)), 12.0, 1.0), Strength::Required));
        let comps = components(&p);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].vars, vec![a, b]);
        assert_eq!(comps[0].backend, Backend::Linear);
        assert_eq!(comps[0].rules, vec![0, 1]);
        assert_eq!(comps[1].vars, vec![c, d]);
        assert_eq!(comps[1].backend, Backend::Numeric);
    }

    #[test]
    fn constant_rule_is_its_own_component() {
        let mut p = Problem::default();
        let f = p.add_var(Variable::new(5.0).fixed(true));
        p.rules.push(Rule::new(1, rules::fix(Expr::Var(f), 6.0, 1.0), Strength::Required));
        let comps = components(&p);
        assert_eq!(comps.len(), 1);
        assert!(comps[0].vars.is_empty());
        assert_eq!(comps[0].backend, Backend::Trivial);
    }
}
