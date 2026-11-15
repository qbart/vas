use vas::Aabb;

#[test]
fn construction_and_measures() {
    let b = Aabb::new([0.0, 1.0], [4.0, 3.0]);
    assert_eq!(b.center(), [2.0, 2.0]);
    assert_eq!(b.size(), [4.0, 2.0]);
    let p = Aabb::point([1.0, 2.0, 3.0]);
    assert_eq!(p.min, p.max);
    assert_eq!(p.size(), [0.0; 3]);
    // Corners given in the wrong order are sorted per axis.
    assert_eq!(Aabb::new([4.0, 1.0], [0.0, 3.0]), b);
    assert_eq!(
        Aabb::around([2.0, 2.0], 1.0),
        Aabb::new([1.0, 1.0], [3.0, 3.0])
    );
}

#[test]
fn point_tests() {
    let b = Aabb::new([0.0, 0.0], [2.0, 2.0]);
    assert!(b.contains_point(&[1.0, 1.0]));
    assert!(b.contains_point(&[0.0, 2.0])); // edges are inside
    assert!(!b.contains_point(&[2.1, 1.0]));
    assert!(!b.contains_point(&[f32::NAN, 1.0]));
    assert_eq!(b.distance_squared(&[1.0, 1.0]), 0.0);
    assert_eq!(b.distance_squared(&[5.0, 6.0]), 25.0);
    assert_eq!(b.distance_squared(&[-1.0, 1.0]), 1.0);
}

#[test]
fn box_tests() {
    let a = Aabb::new([0.0, 0.0], [2.0, 2.0]);
    assert!(a.intersects(&Aabb::new([1.0, 1.0], [3.0, 3.0])));
    assert!(a.intersects(&Aabb::new([2.0, 0.0], [3.0, 1.0]))); // touching counts
    assert!(!a.intersects(&Aabb::new([2.1, 0.0], [3.0, 1.0])));
    assert!(!a.intersects(&Aabb::new([0.0, 3.0], [1.0, 4.0])));
    assert!(a.contains(&Aabb::new([0.5, 0.5], [1.5, 2.0])));
    assert!(!a.contains(&Aabb::new([0.5, 0.5], [2.5, 1.0])));
    assert_eq!(
        a.union(&Aabb::new([-1.0, 1.0], [1.0, 5.0])),
        Aabb::new([-1.0, 0.0], [2.0, 5.0])
    );
    assert_eq!(a.grown(&[3.0, -1.0]), Aabb::new([0.0, -1.0], [3.0, 2.0]));
    assert_eq!(a.expanded(1.0), Aabb::new([-1.0, -1.0], [3.0, 3.0]));
}
