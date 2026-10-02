//! Every rule builder's exact Jacobian is checked against central differences
//! (DL-SOLVE-5) at random, non-degenerate configurations.

use dotloom_constraints::{
    Expr, PointExpr, Row, VarId,
    rules::{self, Cmp},
};
use proptest::prelude::*;

fn p(i: u32) -> PointExpr {
    PointExpr::vars(VarId(2 * i), VarId(2 * i + 1))
}

fn v(i: u32) -> Expr {
    Expr::Var(VarId(i))
}

fn all_rows() -> Vec<(&'static str, Vec<Row>)> {
    let s = 1000.0;
    vec![
        ("fix", rules::fix(v(0), 3.0, s)),
        ("equal", rules::equal(v(0), v(1), s)),
        ("linear", rules::linear(&[(2.0, v(0)), (-1.5, v(3))], Cmp::Ge, 4.0, s)),
        ("ratio", rules::ratio(v(0), v(5), 0.75, s)),
        ("spacing", rules::equal_spacing(&[v(0), v(2), v(4), v(6)], s)),
        ("coincident", rules::coincident(&p(0), &p(1), s)),
        ("horizontal", rules::horizontal(&p(0), &p(1), s)),
        ("vertical", rules::vertical(&p(0), &p(1), s)),
        ("fix_point", rules::fix_point(&p(2), (1.0, -2.0), s)),
        ("distance", rules::distance(&p(0), &p(1), 7.0, s)),
        ("distance_ge", rules::distance_cmp(&p(0), &p(1), Cmp::Ge, 7.0, s)),
        ("point_line_distance", rules::point_line_distance(&p(2), &p(0), &p(1), -3.0, s)),
        ("point_on_line", rules::point_on_line(&p(2), &p(0), &p(1), s)),
        ("point_on_circle", rules::point_on_circle(&p(2), &p(0), v(8), s)),
        ("equal_length", rules::equal_length(&p(0), &p(1), &p(2), &p(3), s)),
        ("parallel", rules::parallel(&p(0), &p(1), &p(2), &p(3))),
        ("perpendicular", rules::perpendicular(&p(0), &p(1), &p(2), &p(3))),
        ("angle", rules::angle(&p(0), &p(1), &p(2), &p(3), 0.6)),
        ("tangent_line_circle", rules::tangent_line_circle(&p(0), &p(1), &p(2), v(8), 1.0, s)),
        ("tangent_circles_ext", rules::tangent_circles(&p(0), v(8), &p(3), v(9), rules::CircleTangency::External, s)),
        (
            "tangent_circles_int",
            rules::tangent_circles(&p(0), v(8), &p(3), v(9), rules::CircleTangency::Internal { sign: -1.0 }, s),
        ),
        ("polar_point", rules::coincident(&rules::polar_point(v(0), v(1), v(8), v(9)), &p(3), s)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    #[test]
    fn rule_jacobians_match_central_differences(x in prop::collection::vec(-50.0..50.0f64, 10)) {
        // Keep radii positive and points apart to stay away from non-smooth points.
        let mut x = x;
        x[8] = x[8].abs() + 1.0;
        x[9] = x[9].abs() + 1.0;
        let d = |a: f64, b: f64| (a * a + b * b).sqrt();
        if d(x[0] - x[2], x[1] - x[3]) < 1.0 || d(x[4] - x[6], x[5] - x[7]) < 1.0 {
            return Ok(());
        }
        for (name, rows) in all_rows() {
            for row in rows {
                let d = row.expr.eval_dual(&x);
                prop_assert!((d.v - row.expr.eval(&x)).abs() <= 1e-12 * (1.0 + d.v.abs()));
                for i in 0..x.len() {
                    let h = 1e-6 * (1.0 + x[i].abs());
                    let mut xp = x.clone();
                    let mut xm = x.clone();
                    xp[i] += h;
                    xm[i] -= h;
                    let num = (row.expr.eval(&xp) - row.expr.eval(&xm)) / (2.0 * h);
                    let ana = d.g.iter().find(|g| g.0 as usize == i).map_or(0.0, |g| g.1);
                    prop_assert!(
                        (num - ana).abs() <= 1e-5 * (1.0 + num.abs()),
                        "{name} d/dx{i}: analytic {ana} vs numeric {num} at {x:?}"
                    );
                }
            }
        }
    }
}
