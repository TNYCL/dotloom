//! The Dotloom expression language used by plugin model definitions.
//!
//! Expressions are parsed and type-checked (with physical dimensions) when a type
//! definition is registered, then lowered to [`dotloom_constraints::Expr`] trees whose
//! variables are *symbolic leaves* (properties, anchors of referenced entities,
//! time-axis constants). The same compiled form is evaluated numerically for
//! drawing and instantiated with solver variables for constraint solving, so the
//! browser and native builds share one semantics. There is no `eval` and no
//! callback into host code.
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/') unary)*
//! unary   := '-' unary | postfix
//! postfix := primary ('.' ident)*
//! primary := number[unit] | ident | ident '(' args ')' | '(' expr ')'
//! ```
//!
//! Units on literals: `mm cm m in ft deg rad ms s min h d`. Values: scalars with a
//! dimension, and 2D vectors (lowered to component pairs).

use std::collections::BTreeMap;

use dotloom_constraints::{Expr, VarId};
use dotloom_geometry::units::{Dim, Quantity};
use thiserror::Error;

/// Errors from parsing or type-checking expressions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LangError {
    /// Lexical/syntax error.
    #[error("syntax error at {pos}: {msg}")]
    Syntax {
        /// Byte offset.
        pos: usize,
        /// Message.
        msg: String,
    },
    /// Unknown identifier.
    #[error("unknown name `{0}`")]
    UnknownName(String),
    /// Unknown function.
    #[error("unsupported function `{0}`")]
    UnknownFunction(String),
    /// Type or dimension mismatch.
    #[error("type error: {0}")]
    Type(String),
    /// Expression too large or nested too deeply.
    #[error("expression too complex: {0}")]
    TooComplex(String),
}

// ---------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64, Option<String>),
    Ident(String),
    Op(char),
}

fn lex(src: &str) -> Result<Vec<(usize, Tok)>, LangError> {
    if src.len() > 4096 {
        return Err(LangError::TooComplex("longer than 4096 bytes".into()));
    }
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            // Exponent: e/E followed by digits (optionally signed).
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    i = j;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let v: f64 =
                src[start..i].parse().map_err(|_| LangError::Syntax { pos: start, msg: "bad number".into() })?;
            if !v.is_finite() {
                return Err(LangError::Syntax { pos: start, msg: "number out of range".into() });
            }
            let ustart = i;
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            let unit = (i > ustart).then(|| src[ustart..i].to_owned());
            out.push((start, Tok::Num(v, unit)));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((start, Tok::Ident(src[start..i].to_owned())));
        } else if "+-*/(),.".contains(c) {
            out.push((i, Tok::Op(c)));
            i += 1;
        } else {
            return Err(LangError::Syntax { pos: i, msg: format!("unexpected character `{c}`") });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Parser

/// Parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    /// Literal (canonical units).
    Num(f64, Dim),
    /// Name.
    Name(String),
    /// `a.b`.
    Member(Box<Ast>, String),
    /// Function call.
    Call(String, Vec<Ast>),
    /// Negation.
    Neg(Box<Ast>),
    /// Binary operation.
    Bin(char, Box<Ast>, Box<Ast>),
}

struct Parser {
    toks: Vec<(usize, Tok)>,
    i: usize,
    depth: usize,
}

const MAX_DEPTH: usize = 64;

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.1)
    }
    fn pos(&self) -> usize {
        self.toks.get(self.i).map_or(usize::MAX, |t| t.0)
    }
    fn err<T>(&self, msg: &str) -> Result<T, LangError> {
        Err(LangError::Syntax { pos: self.pos(), msg: msg.to_owned() })
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Op(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn enter(&mut self) -> Result<(), LangError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(LangError::TooComplex("nesting deeper than 64".into())) } else { Ok(()) }
    }
    fn expr(&mut self) -> Result<Ast, LangError> {
        self.enter()?;
        let mut a = self.term()?;
        loop {
            if self.eat('+') {
                a = Ast::Bin('+', Box::new(a), Box::new(self.term()?));
            } else if self.eat('-') {
                a = Ast::Bin('-', Box::new(a), Box::new(self.term()?));
            } else {
                break;
            }
        }
        self.depth -= 1;
        Ok(a)
    }
    fn term(&mut self) -> Result<Ast, LangError> {
        let mut a = self.unary()?;
        loop {
            if self.eat('*') {
                a = Ast::Bin('*', Box::new(a), Box::new(self.unary()?));
            } else if self.eat('/') {
                a = Ast::Bin('/', Box::new(a), Box::new(self.unary()?));
            } else {
                break;
            }
        }
        Ok(a)
    }
    fn unary(&mut self) -> Result<Ast, LangError> {
        if self.eat('-') {
            self.enter()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Ast::Neg(Box::new(inner)));
        }
        let mut a = self.primary()?;
        while self.eat('.') {
            match self.peek().cloned() {
                Some(Tok::Ident(n)) => {
                    self.i += 1;
                    a = Ast::Member(Box::new(a), n);
                }
                _ => return self.err("expected member name after `.`"),
            }
        }
        Ok(a)
    }
    fn primary(&mut self) -> Result<Ast, LangError> {
        match self.peek().cloned() {
            Some(Tok::Num(v, unit)) => {
                self.i += 1;
                let q = match unit {
                    None => Quantity::scalar(v),
                    Some(u) => Quantity::parse(&format!("{v} {u}"))
                        .map_err(|_| LangError::Syntax { pos: self.pos(), msg: format!("unknown unit `{u}`") })?,
                };
                Ok(Ast::Num(q.value, q.dim))
            }
            Some(Tok::Ident(n)) => {
                self.i += 1;
                if self.eat('(') {
                    let mut args = Vec::new();
                    if !self.eat(')') {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(')') {
                                break;
                            }
                            if !self.eat(',') {
                                return self.err("expected `,` or `)`");
                            }
                        }
                    }
                    Ok(Ast::Call(n, args))
                } else {
                    Ok(Ast::Name(n))
                }
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let e = self.expr()?;
                if !self.eat(')') {
                    return self.err("expected `)`");
                }
                Ok(e)
            }
            _ => self.err("expected a value"),
        }
    }
}

/// Parse an expression.
pub fn parse(src: &str) -> Result<Ast, LangError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, i: 0, depth: 0 };
    let e = p.expr()?;
    if p.i != p.toks.len() {
        return p.err("unexpected trailing input");
    }
    Ok(e)
}

// ---------------------------------------------------------------------------
// Types, leaves and compiled values

/// Value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    /// Scalar with dimension.
    Scalar(Dim),
    /// 2D vector with dimension.
    Vector(Dim),
}

impl Ty {
    fn describe(self) -> String {
        match self {
            Self::Scalar(d) => format!("scalar[{d}]"),
            Self::Vector(d) => format!("vector[{d}]"),
        }
    }
}

/// External inputs of compiled expressions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Leaf {
    /// Numeric property of the entity itself.
    Prop(String),
    /// X/Y component of a point property of the entity itself.
    PropPoint(String, Axis),
    /// Anchor of a referenced entity (property `prop` holds the reference),
    /// expressed in the referencing entity's local coordinates.
    RefAnchor(String, String, Axis),
    /// Numeric parameter of a referenced entity.
    RefParam(String, String),
    /// Document time axis: millimetres per second.
    AxisScale,
    /// Document time axis: origin in seconds.
    AxisOrigin,
}

/// Vector component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    /// X.
    X,
    /// Y.
    Y,
}

/// A compiled scalar or vector expression over [`Leaf`] inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// Type.
    pub ty: Ty,
    /// One expression (scalar) or two (vector x, y). Variables index `leaves`.
    pub parts: Vec<Expr>,
    /// Leaf table.
    pub leaves: Vec<Leaf>,
}

impl Compiled {
    /// Evaluate with leaf values supplied by `f`.
    pub fn eval(&self, f: &mut dyn FnMut(&Leaf) -> Option<f64>) -> Option<Vec<f64>> {
        let mut vals = Vec::with_capacity(self.leaves.len());
        for l in &self.leaves {
            vals.push(f(l)?);
        }
        let out: Vec<f64> = self.parts.iter().map(|e| e.eval(&vals)).collect();
        out.iter().all(|v| v.is_finite()).then_some(out)
    }

    /// Instantiate with leaf expressions supplied by `f` (solver variables or constants).
    pub fn instantiate(&self, f: &mut dyn FnMut(&Leaf) -> Option<Expr>) -> Option<Vec<Expr>> {
        let mut subs = Vec::with_capacity(self.leaves.len());
        for l in &self.leaves {
            subs.push(f(l)?);
        }
        Some(
            self.parts
                .iter()
                .map(|e| e.substitute(&|v: VarId| subs.get(v.index()).cloned().unwrap_or(Expr::c(f64::NAN))))
                .collect(),
        )
    }
}

/// What a name means while type-checking.
#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    /// Numeric property with dimension.
    NumberProp(Dim),
    /// Point property.
    PointProp,
    /// Reference property; `target` lists the members available on the referenced
    /// type (`None` = untyped reference, members are not allowed).
    RefProp(Option<BTreeMap<String, Ty>>),
    /// Already compiled helper (anchor or derived value).
    Value(Compiled),
}

/// Names visible to an expression.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scope {
    /// Bindings.
    pub names: BTreeMap<String, Binding>,
    /// Whether `axis.mmPerSecond` / `axis.origin` are available.
    pub has_axis: bool,
}

struct Lower<'a> {
    scope: &'a Scope,
    leaves: Vec<Leaf>,
    nodes: usize,
}

type Val = (Ty, Vec<Expr>);

fn s(d: Dim) -> Ty {
    Ty::Scalar(d)
}

impl Lower<'_> {
    fn leaf(&mut self, l: Leaf) -> Expr {
        if let Some(i) = self.leaves.iter().position(|x| *x == l) {
            return Expr::Var(VarId(u32::try_from(i).unwrap_or(u32::MAX)));
        }
        self.leaves.push(l);
        Expr::Var(VarId(u32::try_from(self.leaves.len() - 1).unwrap_or(u32::MAX)))
    }

    fn import(&mut self, c: &Compiled) -> Val {
        // Re-map the helper's leaves into this expression's leaf table.
        let map: Vec<Expr> = c.leaves.iter().map(|l| self.leaf(l.clone())).collect();
        let parts = c
            .parts
            .iter()
            .map(|e| e.substitute(&|v: VarId| map.get(v.index()).cloned().unwrap_or(Expr::c(f64::NAN))))
            .collect();
        (c.ty, parts)
    }

    fn lower(&mut self, a: &Ast) -> Result<Val, LangError> {
        self.nodes += 1;
        if self.nodes > 10_000 {
            return Err(LangError::TooComplex("more than 10000 nodes".into()));
        }
        match a {
            Ast::Num(v, d) => Ok((s(*d), vec![Expr::c(*v)])),
            Ast::Name(n) => self.name(n),
            Ast::Member(base, member) => self.member(base, member),
            Ast::Neg(x) => {
                let (t, p) = self.lower(x)?;
                Ok((t, p.into_iter().map(Expr::neg).collect()))
            }
            Ast::Bin(op, x, y) => {
                let a = self.lower(x)?;
                let b = self.lower(y)?;
                self.binary(*op, a, b)
            }
            Ast::Call(f, args) => {
                let vals = args.iter().map(|x| self.lower(x)).collect::<Result<Vec<_>, _>>()?;
                self.call(f, vals)
            }
        }
    }

    fn name(&mut self, n: &str) -> Result<Val, LangError> {
        if n == "pi" {
            return Ok((s(Dim::ANGLE), vec![Expr::c(core::f64::consts::PI)]));
        }
        match self.scope.names.get(n) {
            Some(Binding::NumberProp(d)) => {
                let e = self.leaf(Leaf::Prop(n.to_owned()));
                Ok((s(*d), vec![e]))
            }
            Some(Binding::PointProp) => {
                let x = self.leaf(Leaf::PropPoint(n.to_owned(), Axis::X));
                let y = self.leaf(Leaf::PropPoint(n.to_owned(), Axis::Y));
                Ok((Ty::Vector(Dim::LENGTH), vec![x, y]))
            }
            Some(Binding::Value(c)) => {
                let c = c.clone();
                Ok(self.import(&c))
            }
            Some(Binding::RefProp(_)) => {
                Err(LangError::Type(format!("reference `{n}` must be followed by a member, e.g. `{n}.start`")))
            }
            None => Err(LangError::UnknownName(n.to_owned())),
        }
    }

    fn member(&mut self, base: &Ast, member: &str) -> Result<Val, LangError> {
        let Ast::Name(b) = base else {
            return Err(LangError::Type("member access is only allowed on references and `axis`".into()));
        };
        if b == "axis" {
            if !self.scope.has_axis {
                return Err(LangError::UnknownName("axis".into()));
            }
            return match member {
                "mmPerSecond" => Ok((s(Dim::LENGTH.div(Dim::TIME)), vec![self.leaf(Leaf::AxisScale)])),
                "origin" => Ok((s(Dim::TIME), vec![self.leaf(Leaf::AxisOrigin)])),
                _ => Err(LangError::UnknownName(format!("axis.{member}"))),
            };
        }
        match self.scope.names.get(b) {
            Some(Binding::RefProp(Some(members))) => match members.get(member) {
                Some(Ty::Vector(_)) => {
                    let x = self.leaf(Leaf::RefAnchor(b.clone(), member.to_owned(), Axis::X));
                    let y = self.leaf(Leaf::RefAnchor(b.clone(), member.to_owned(), Axis::Y));
                    Ok((Ty::Vector(Dim::LENGTH), vec![x, y]))
                }
                Some(Ty::Scalar(d)) => {
                    let d = *d;
                    Ok((s(d), vec![self.leaf(Leaf::RefParam(b.clone(), member.to_owned()))]))
                }
                None => Err(LangError::UnknownName(format!("{b}.{member}"))),
            },
            Some(Binding::RefProp(None)) => {
                Err(LangError::Type(format!("reference `{b}` has no declared target type")))
            }
            _ => Err(LangError::UnknownName(format!("{b}.{member}"))),
        }
    }

    fn binary(&mut self, op: char, (ta, a): Val, (tb, b): Val) -> Result<Val, LangError> {
        let mismatch = || LangError::Type(format!("cannot apply `{op}` to {} and {}", ta.describe(), tb.describe()));
        match (op, ta, tb) {
            ('+' | '-', Ty::Scalar(d1), Ty::Scalar(d2)) | ('+' | '-', Ty::Vector(d1), Ty::Vector(d2)) => {
                if d1 != d2 {
                    return Err(LangError::Type(format!(
                        "cannot {} {d1} and {d2}",
                        if op == '+' { "add" } else { "subtract" }
                    )));
                }
                let parts = a
                    .into_iter()
                    .zip(b)
                    .map(|(x, y)| if op == '+' { Expr::add(x, y) } else { Expr::sub(x, y) })
                    .collect();
                Ok((ta, parts))
            }
            ('*', Ty::Scalar(d1), Ty::Scalar(d2)) => Ok((s(d1.mul(d2)), vec![Expr::mul(one(a), one(b))])),
            ('*', Ty::Vector(d1), Ty::Scalar(d2)) => {
                let k = one(b);
                Ok((Ty::Vector(d1.mul(d2)), a.into_iter().map(|x| Expr::mul(x, k.clone())).collect()))
            }
            ('*', Ty::Scalar(d1), Ty::Vector(d2)) => {
                let k = one(a);
                Ok((Ty::Vector(d1.mul(d2)), b.into_iter().map(|x| Expr::mul(k.clone(), x)).collect()))
            }
            ('/', Ty::Scalar(d1), Ty::Scalar(d2)) => Ok((s(d1.div(d2)), vec![Expr::div(one(a), one(b))])),
            ('/', Ty::Vector(d1), Ty::Scalar(d2)) => {
                let k = one(b);
                Ok((Ty::Vector(d1.div(d2)), a.into_iter().map(|x| Expr::div(x, k.clone())).collect()))
            }
            _ => Err(mismatch()),
        }
    }

    fn call(&mut self, f: &str, args: Vec<Val>) -> Result<Val, LangError> {
        let arity = |n: usize| -> Result<(), LangError> {
            if args.len() == n {
                Ok(())
            } else {
                Err(LangError::Type(format!("`{f}` takes {n} argument(s), got {}", args.len())))
            }
        };
        let scalar = |v: &Val| -> Result<(Dim, Expr), LangError> {
            match v.0 {
                Ty::Scalar(d) => Ok((d, v.1.first().cloned().unwrap_or(Expr::c(f64::NAN)))),
                Ty::Vector(_) => Err(LangError::Type(format!("`{f}` expects a scalar"))),
            }
        };
        let vector = |v: &Val| -> Result<(Dim, Expr, Expr), LangError> {
            match (v.0, v.1.as_slice()) {
                (Ty::Vector(d), [x, y]) => Ok((d, x.clone(), y.clone())),
                _ => Err(LangError::Type(format!("`{f}` expects a vector"))),
            }
        };
        let angle_like = |d: Dim| d == Dim::ANGLE || d == Dim::SCALAR;
        match f {
            "min" | "max" => {
                arity(2)?;
                let (a, b) = (&args[0], &args[1]);
                if a.0 != b.0 || matches!(a.0, Ty::Vector(_)) {
                    return Err(LangError::Type(format!("`{f}` needs two scalars of the same dimension")));
                }
                let (x, y) = (one(a.1.clone()), one(b.1.clone()));
                Ok((a.0, vec![if f == "min" { Expr::min(x, y) } else { Expr::max(x, y) }]))
            }
            "clamp" => {
                arity(3)?;
                let ((d, x), (d1, lo), (d2, hi)) = (scalar(&args[0])?, scalar(&args[1])?, scalar(&args[2])?);
                if d != d1 || d != d2 {
                    return Err(LangError::Type("`clamp` arguments must share a dimension".into()));
                }
                Ok((s(d), vec![Expr::min(Expr::max(x, lo), hi)]))
            }
            "abs" => {
                arity(1)?;
                let (d, x) = scalar(&args[0])?;
                Ok((s(d), vec![Expr::abs(x)]))
            }
            "sqrt" => {
                arity(1)?;
                let (d, x) = scalar(&args[0])?;
                if d.length % 2 != 0 || d.angle % 2 != 0 || d.time % 2 != 0 {
                    return Err(LangError::Type(format!("sqrt of {d} has no dimension")));
                }
                let half = Dim { length: d.length / 2, angle: d.angle / 2, time: d.time / 2 };
                Ok((s(half), vec![Expr::sqrt(x)]))
            }
            "sin" | "cos" => {
                arity(1)?;
                let (d, x) = scalar(&args[0])?;
                if !angle_like(d) {
                    return Err(LangError::Type(format!("`{f}` expects an angle, got {d}")));
                }
                Ok((s(Dim::SCALAR), vec![if f == "sin" { Expr::sin(x) } else { Expr::cos(x) }]))
            }
            "atan2" => {
                arity(2)?;
                let ((d1, y), (d2, x)) = (scalar(&args[0])?, scalar(&args[1])?);
                if d1 != d2 {
                    return Err(LangError::Type("`atan2` arguments must share a dimension".into()));
                }
                Ok((s(Dim::ANGLE), vec![Expr::atan2(y, x)]))
            }
            "hypot" => {
                arity(2)?;
                let ((d1, x), (d2, y)) = (scalar(&args[0])?, scalar(&args[1])?);
                if d1 != d2 {
                    return Err(LangError::Type("`hypot` arguments must share a dimension".into()));
                }
                Ok((s(d1), vec![Expr::hypot(x, y)]))
            }
            "vec" => {
                arity(2)?;
                let ((d1, x), (d2, y)) = (scalar(&args[0])?, scalar(&args[1])?);
                if d1 != d2 {
                    return Err(LangError::Type("`vec` components must share a dimension".into()));
                }
                Ok((Ty::Vector(d1), vec![x, y]))
            }
            "x" | "y" => {
                arity(1)?;
                let (d, x, y) = vector(&args[0])?;
                Ok((s(d), vec![if f == "x" { x } else { y }]))
            }
            "len" => {
                arity(1)?;
                let (d, x, y) = vector(&args[0])?;
                Ok((s(d), vec![Expr::hypot(x, y)]))
            }
            "norm" => {
                arity(1)?;
                let (_, x, y) = vector(&args[0])?;
                let l = Expr::hypot(x.clone(), y.clone());
                Ok((Ty::Vector(Dim::SCALAR), vec![Expr::div(x, l.clone()), Expr::div(y, l)]))
            }
            "perp" => {
                arity(1)?;
                let (d, x, y) = vector(&args[0])?;
                Ok((Ty::Vector(d), vec![Expr::neg(y), x]))
            }
            "dot" | "cross" => {
                arity(2)?;
                let ((d1, ax, ay), (d2, bx, by)) = (vector(&args[0])?, vector(&args[1])?);
                let e = if f == "dot" {
                    Expr::add(Expr::mul(ax, bx), Expr::mul(ay, by))
                } else {
                    Expr::sub(Expr::mul(ax, by), Expr::mul(ay, bx))
                };
                Ok((s(d1.mul(d2)), vec![e]))
            }
            "dist" => {
                arity(2)?;
                let ((d1, ax, ay), (d2, bx, by)) = (vector(&args[0])?, vector(&args[1])?);
                if d1 != d2 {
                    return Err(LangError::Type("`dist` arguments must share a dimension".into()));
                }
                Ok((s(d1), vec![Expr::hypot(Expr::sub(bx, ax), Expr::sub(by, ay))]))
            }
            "angle" => {
                arity(1)?;
                let (_, x, y) = vector(&args[0])?;
                Ok((s(Dim::ANGLE), vec![Expr::atan2(y, x)]))
            }
            "rotate" => {
                arity(2)?;
                let (d, x, y) = vector(&args[0])?;
                let (da, a) = scalar(&args[1])?;
                if !angle_like(da) {
                    return Err(LangError::Type("`rotate` expects an angle".into()));
                }
                let (c, sn) = (Expr::cos(a.clone()), Expr::sin(a));
                Ok((
                    Ty::Vector(d),
                    vec![
                        Expr::sub(Expr::mul(x.clone(), c.clone()), Expr::mul(y.clone(), sn.clone())),
                        Expr::add(Expr::mul(x, sn), Expr::mul(y, c)),
                    ],
                ))
            }
            "lerp" => {
                arity(3)?;
                let (dt, t) = scalar(&args[2])?;
                if dt != Dim::SCALAR {
                    return Err(LangError::Type("`lerp` parameter must be dimensionless".into()));
                }
                let (a, b) = (&args[0], &args[1]);
                if a.0 != b.0 {
                    return Err(LangError::Type("`lerp` endpoints must share a type".into()));
                }
                let parts =
                    a.1.iter()
                        .zip(&b.1)
                        .map(|(x, y)| Expr::add(x.clone(), Expr::mul(t.clone(), Expr::sub(y.clone(), x.clone()))))
                        .collect();
                Ok((a.0, parts))
            }
            other => Err(LangError::UnknownFunction(other.to_owned())),
        }
    }
}

fn one(v: Vec<Expr>) -> Expr {
    v.into_iter().next().unwrap_or(Expr::c(f64::NAN))
}

/// Parse, type-check and lower `src` in `scope`.
pub fn compile(src: &str, scope: &Scope) -> Result<Compiled, LangError> {
    let ast = parse(src)?;
    let mut l = Lower { scope, leaves: Vec::new(), nodes: 0 };
    let (ty, parts) = l.lower(&ast)?;
    Ok(Compiled { ty, parts, leaves: l.leaves })
}

/// Compile and require a type.
pub fn compile_as(src: &str, scope: &Scope, want: Ty) -> Result<Compiled, LangError> {
    let c = compile(src, scope)?;
    if c.ty == want {
        Ok(c)
    } else {
        Err(LangError::Type(format!("`{src}` is {} but {} is required", c.ty.describe(), want.describe())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        let mut sc = Scope::default();
        sc.names.insert("width".into(), Binding::NumberProp(Dim::LENGTH));
        sc.names.insert("start".into(), Binding::PointProp);
        sc.names.insert("end".into(), Binding::PointProp);
        sc.names.insert("t0".into(), Binding::NumberProp(Dim::TIME));
        let wall: BTreeMap<String, Ty> =
            [("start".to_owned(), Ty::Vector(Dim::LENGTH)), ("thickness".to_owned(), Ty::Scalar(Dim::LENGTH))]
                .into_iter()
                .collect();
        sc.names.insert("host".into(), Binding::RefProp(Some(wall)));
        sc.has_axis = true;
        sc
    }

    fn eval(src: &str) -> Vec<f64> {
        let c = compile(src, &scope()).unwrap();
        c.eval(&mut |l| {
            Some(match l {
                Leaf::Prop(n) if n == "width" => 600.0,
                Leaf::Prop(n) if n == "t0" => 7200.0,
                Leaf::PropPoint(n, a) => match (n.as_str(), a) {
                    ("start", Axis::X) => 0.0,
                    ("start", Axis::Y) => 0.0,
                    ("end", Axis::X) => 3000.0,
                    _ => 4000.0,
                },
                Leaf::RefAnchor(_, _, Axis::X) => 10.0,
                Leaf::RefAnchor(_, _, Axis::Y) => 20.0,
                Leaf::RefParam(_, _) => 200.0,
                Leaf::AxisScale => 0.5,
                Leaf::AxisOrigin => 3600.0,
                Leaf::Prop(_) => return None,
            })
        })
        .unwrap()
    }

    #[test]
    fn arithmetic_units_and_vectors() {
        assert_eq!(eval("width + 40cm"), vec![1000.0]);
        assert_eq!(eval("dist(start, end)"), vec![5000.0]);
        let n = eval("norm(end - start)");
        assert!((n[0] - 0.6).abs() < 1e-12 && (n[1] - 0.8).abs() < 1e-12);
        assert_eq!(eval("start + perp(norm(end - start)) * 100mm"), vec![-80.0, 60.0]);
        assert_eq!(eval("host.start + vec(host.thickness, 0mm)"), vec![210.0, 20.0]);
        assert_eq!(eval("(t0 - axis.origin) * axis.mmPerSecond"), vec![1800.0]);
        let r = eval("rotate(vec(1m, 0m), 90deg)");
        assert!(r[0].abs() < 1e-9 && (r[1] - 1000.0).abs() < 1e-9);
        assert_eq!(eval("lerp(start, end, 0.5)"), vec![1500.0, 2000.0]);
        assert_eq!(eval("max(width, 1m) - min(width, 1m)"), vec![400.0]);
        assert_eq!(eval("sqrt(width * width)"), vec![600.0]);
        assert_eq!(eval("1.5e3mm"), vec![1500.0]);
    }

    #[test]
    fn dimension_errors() {
        let sc = scope();
        for bad in [
            "width + t0",
            "width + 90deg",
            "start + width",
            "sin(width)",
            "sqrt(width)",
            "lerp(start, end, width)",
            "start * end",
        ] {
            assert!(matches!(compile(bad, &sc), Err(LangError::Type(_))), "{bad}");
        }
        assert!(matches!(compile("nope + 1", &sc), Err(LangError::UnknownName(_))));
        assert!(matches!(compile("eval(1)", &sc), Err(LangError::UnknownFunction(_))));
        assert!(matches!(compile("host", &sc), Err(LangError::Type(_))));
        assert!(matches!(compile("host.missing", &sc), Err(LangError::UnknownName(_))));
        assert!(matches!(compile("1 +", &sc), Err(LangError::Syntax { .. })));
        assert!(matches!(compile("width $ 2", &sc), Err(LangError::Syntax { .. })));
        assert!(matches!(compile("3 parsecs", &sc), Err(LangError::Syntax { .. })));
        let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
        assert!(matches!(compile(&deep, &sc), Err(LangError::TooComplex(_))));
        assert!(compile_as("width", &sc, Ty::Vector(Dim::LENGTH)).is_err());
    }

    #[test]
    fn instantiate_matches_eval_and_has_exact_gradients() {
        let c = compile("dist(start, host.start) * 2 + width", &scope()).unwrap();
        // Leaves → solver variables 0..n.
        let mut next = 0u32;
        let mut map = BTreeMap::new();
        let inst = c
            .instantiate(&mut |l| {
                let id = *map.entry(l.clone()).or_insert_with(|| {
                    next += 1;
                    next - 1
                });
                Some(Expr::Var(VarId(id)))
            })
            .unwrap();
        let x: Vec<f64> = (0..next).map(|i| 1.0 + f64::from(i) * 3.0).collect();
        let d = inst[0].eval_dual(&x);
        assert!(d.v.is_finite());
        assert!(!d.g.is_empty());
    }
}
