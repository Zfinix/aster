use super::*;

#[test]
fn empty_sample_is_all_zero() {
    assert_eq!(Dist::new(Vec::new()), Dist::default());
}

#[test]
fn percentiles_use_nearest_rank() {
    let dist = Dist::new((1..=10).map(f64::from).collect());
    assert_eq!(dist.n, 10);
    assert_eq!(dist.p50, 5.0);
    assert_eq!(dist.p90, 9.0);
    assert_eq!(dist.max, 10.0);
    assert_eq!(dist.total, 55.0);
    assert_eq!(dist.mean, 5.5);
}

#[test]
fn a_single_value_is_every_percentile() {
    let dist = Dist::new(vec![4.0]);
    assert_eq!(
        (dist.p50, dist.p90, dist.p99, dist.max),
        (4.0, 4.0, 4.0, 4.0)
    );
}

#[test]
fn unsorted_input_is_sorted_first() {
    let dist = Dist::new(vec![9.0, 1.0, 5.0]);
    assert_eq!(dist.p50, 5.0);
    assert_eq!(dist.max, 9.0);
}
