//! Overlay layout + drawing benchmarks.
//!
//! Run with `cargo bench --features bench`. Everything draws into an
//! in-memory DIB on the primary monitor's geometry — DWM and the
//! `UpdateLayeredWindow` hand-off are not measured. Absolute numbers
//! depend on the machine's primary monitor (size + DPI); compare
//! variants on the same machine only.

#[cfg(windows)]
mod benches {
    use criterion::{black_box, BenchmarkId, Criterion};
    use keyhop::windows::overlay::bench::{self, Prepared, Surface};
    use keyhop::windows::overlay::{BadgeShape, Hint, HintStyle};

    const COUNTS: [usize; 3] = [50, 300, 800];

    type Scenario = fn(usize) -> Vec<Hint>;

    /// Style variants under test, DPI-scaled the same way a real pick is.
    fn styles() -> Vec<(&'static str, HintStyle)> {
        let scale = bench::dpi_scale();
        let callout = HintStyle::elements().scaled_for_dpi(scale);
        let rect = HintStyle {
            shape: BadgeShape::Rect,
            ..callout
        };
        vec![("rect", rect), ("callout", callout)]
    }

    fn scenarios() -> Vec<(&'static str, Scenario)> {
        vec![
            ("scattered", bench::scattered_hints as Scenario),
            ("list", bench::dense_list_hints),
        ]
    }

    /// Not a timing: prints how each scenario was placed under each
    /// shape, so layout quality can be judged next to the numbers.
    pub fn placement_report(_: &mut Criterion) {
        println!(
            "
placement mix [below, above, slid, side, no-tail] / sprites:"
        );
        for (style_name, style) in styles() {
            for (scenario, make) in scenarios() {
                for n in COUNTS {
                    let p = Prepared::new(make(n), style);
                    println!(
                        "  {style_name}/{scenario}/{n}: {:?} / {}",
                        p.placement_mix(),
                        p.sprite_count()
                    );
                }
            }
        }
    }

    pub fn lay_out(c: &mut Criterion) {
        let mut g = c.benchmark_group("lay_out");
        for (style_name, style) in styles() {
            for (scenario, make) in scenarios() {
                for n in COUNTS {
                    let hints = make(n);
                    g.bench_with_input(
                        BenchmarkId::new(format!("{style_name}/{scenario}"), n),
                        &hints,
                        |b, hints| b.iter(|| bench::lay_out_only(black_box(hints), &style)),
                    );
                }
            }
        }
        g.finish();
    }

    /// Pick-start cost: layout + fonts (+ sprite cache, once callouts exist).
    pub fn prepare(c: &mut Criterion) {
        let mut g = c.benchmark_group("prepare");
        for (style_name, style) in styles() {
            for n in COUNTS {
                let hints = bench::scattered_hints(n);
                g.bench_with_input(
                    BenchmarkId::new(format!("{style_name}/scattered"), n),
                    &hints,
                    |b, hints| b.iter(|| Prepared::new(black_box(hints.clone()), style)),
                );
            }
        }
        g.finish();
    }

    pub fn clear(c: &mut Criterion) {
        let mut g = c.benchmark_group("clear");
        for (w, h) in [(1920, 1080), (5120, 1440)] {
            let mut surface = Surface::new(w, h);
            g.bench_function(format!("{w}x{h}"), |b| b.iter(|| surface.clear()));
        }
        g.finish();
    }

    /// Per-frame draw loop. `all` = initial render; `typed` = re-render
    /// after one keystroke (the per-keystroke path).
    pub fn draw(c: &mut Criterion) {
        let (sw, sh) = bench::primary_size();
        let mut surface = Surface::new(sw, sh);
        let mut g = c.benchmark_group("draw");
        for (style_name, style) in styles() {
            for (scenario, make) in scenarios() {
                for n in COUNTS {
                    let mut prepared = Prepared::new(make(n), style);
                    g.bench_function(
                        BenchmarkId::new(format!("{style_name}/{scenario}/all"), n),
                        |b| b.iter(|| prepared.draw(&mut surface)),
                    );
                    let prefix = prepared.common_prefix();
                    prepared.set_typed(&prefix);
                    g.bench_function(
                        BenchmarkId::new(format!("{style_name}/{scenario}/typed"), n),
                        |b| b.iter(|| prepared.draw(&mut surface)),
                    );
                }
            }
        }
        g.finish();
    }
}

#[cfg(windows)]
criterion::criterion_group!(
    overlay,
    benches::placement_report,
    benches::lay_out,
    benches::prepare,
    benches::clear,
    benches::draw
);
#[cfg(windows)]
criterion::criterion_main!(overlay);

#[cfg(not(windows))]
fn main() {}
