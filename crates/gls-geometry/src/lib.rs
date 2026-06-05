use gls_types::{PatchNode, Point3Data, ToleranceProfile};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn distance(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> Self {
        let norm = self.norm();
        if norm <= f64::EPSILON {
            return Self::default();
        }
        Self::new(self.x / norm, self.y / norm, self.z / norm)
    }

    pub fn angle_deg(self, other: Self) -> f64 {
        let denom = self.norm() * other.norm();
        if denom <= f64::EPSILON {
            return 180.0;
        }
        let cos = (self.dot(other) / denom).clamp(-1.0, 1.0);
        cos.acos().to_degrees()
    }
}

impl From<Point3Data> for Point3 {
    fn from(value: Point3Data) -> Self {
        Self::new(value.x, value.y, value.z)
    }
}

impl From<Point3> for Point3Data {
    fn from(value: Point3) -> Self {
        Point3Data::new(value.x, value.y, value.z)
    }
}

impl std::ops::Add for Point3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::Sub for Point3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::Mul<f64> for Point3 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryHashReport {
    pub bucket_count: usize,
    pub max_bucket_occupancy_observed: usize,
    pub memory_estimate_mb: f64,
    pub guard_triggered: bool,
    pub failure_reason: Option<String>,
}

pub fn build_geometry_hash(nodes: &[PatchNode], profile: &ToleranceProfile) -> GeometryHashReport {
    let mut buckets = BTreeMap::<String, usize>::new();
    let width = profile.distance_bin_width.max(f64::EPSILON);
    for node in nodes {
        let point = Point3::from(node.point);
        let key = format!(
            "{}:{}:{}:{}:{}",
            node.kind.as_str(),
            node.residue,
            quantize(point.x, width),
            quantize(point.y, width),
            quantize(point.z, width)
        );
        *buckets.entry(key).or_default() += 1;
    }
    let max_bucket = buckets.values().copied().max().unwrap_or(0);
    let memory_estimate_mb = estimate_geometry_memory_mb(nodes.len(), buckets.len());
    let guard_triggered =
        max_bucket > profile.max_bucket_occupancy || memory_estimate_mb > profile.max_memory_mb;
    GeometryHashReport {
        bucket_count: buckets.len(),
        max_bucket_occupancy_observed: max_bucket,
        memory_estimate_mb,
        guard_triggered,
        failure_reason: if guard_triggered {
            Some("geometry_hash_guard_triggered".to_string())
        } else {
            None
        },
    }
}

pub fn estimate_seed_triplets(seed_nodes: usize, constraints: usize) -> usize {
    let node_triplets = combinations_3(seed_nodes);
    let constraint_triplets = combinations_3(constraints).max(1);
    node_triplets.saturating_mul(constraint_triplets)
}

pub fn kabsch_rmsd(reference: &[Point3], mobile: &[Point3]) -> Result<f64, String> {
    if reference.len() != mobile.len() {
        return Err("kabsch_point_count_mismatch".to_string());
    }
    if reference.is_empty() {
        return Err("kabsch_requires_points".to_string());
    }
    if reference.len() == 1 {
        return Ok(0.0);
    }

    let ref_centroid = centroid(reference);
    let mob_centroid = centroid(mobile);
    let ref_centered = reference
        .iter()
        .map(|point| *point - ref_centroid)
        .collect::<Vec<_>>();
    let mob_centered = mobile
        .iter()
        .map(|point| *point - mob_centroid)
        .collect::<Vec<_>>();
    let rotation = horn_rotation(&mob_centered, &ref_centered);
    let mut squared = 0.0;
    for (mob, reference_point) in mob_centered.iter().zip(&ref_centered) {
        let rotated = rotate(*mob, rotation);
        squared += rotated.distance(*reference_point).powi(2);
    }
    Ok((squared / reference.len() as f64).sqrt())
}

fn centroid(points: &[Point3]) -> Point3 {
    let sum = points
        .iter()
        .copied()
        .fold(Point3::default(), |acc, point| acc + point);
    sum * (1.0 / points.len() as f64)
}

fn horn_rotation(mobile: &[Point3], reference: &[Point3]) -> [f64; 4] {
    let mut s = [[0.0; 3]; 3];
    for (mob, reference_point) in mobile.iter().zip(reference) {
        s[0][0] += mob.x * reference_point.x;
        s[0][1] += mob.x * reference_point.y;
        s[0][2] += mob.x * reference_point.z;
        s[1][0] += mob.y * reference_point.x;
        s[1][1] += mob.y * reference_point.y;
        s[1][2] += mob.y * reference_point.z;
        s[2][0] += mob.z * reference_point.x;
        s[2][1] += mob.z * reference_point.y;
        s[2][2] += mob.z * reference_point.z;
    }
    let trace = s[0][0] + s[1][1] + s[2][2];
    let n = [
        [
            trace,
            s[1][2] - s[2][1],
            s[2][0] - s[0][2],
            s[0][1] - s[1][0],
        ],
        [
            s[1][2] - s[2][1],
            s[0][0] - s[1][1] - s[2][2],
            s[0][1] + s[1][0],
            s[0][2] + s[2][0],
        ],
        [
            s[2][0] - s[0][2],
            s[0][1] + s[1][0],
            -s[0][0] + s[1][1] - s[2][2],
            s[1][2] + s[2][1],
        ],
        [
            s[0][1] - s[1][0],
            s[0][2] + s[2][0],
            s[1][2] + s[2][1],
            -s[0][0] - s[1][1] + s[2][2],
        ],
    ];
    let mut q = [1.0, 0.0, 0.0, 0.0];
    for _ in 0..32 {
        let next = [
            n[0][0] * q[0] + n[0][1] * q[1] + n[0][2] * q[2] + n[0][3] * q[3],
            n[1][0] * q[0] + n[1][1] * q[1] + n[1][2] * q[2] + n[1][3] * q[3],
            n[2][0] * q[0] + n[2][1] * q[1] + n[2][2] * q[2] + n[2][3] * q[3],
            n[3][0] * q[0] + n[3][1] * q[1] + n[3][2] * q[2] + n[3][3] * q[3],
        ];
        let norm = next.iter().map(|value| value * value).sum::<f64>().sqrt();
        if norm <= f64::EPSILON {
            break;
        }
        q = [
            next[0] / norm,
            next[1] / norm,
            next[2] / norm,
            next[3] / norm,
        ];
    }
    q
}

fn rotate(point: Point3, q: [f64; 4]) -> Point3 {
    let [w, x, y, z] = q;
    let m00 = 1.0 - 2.0 * (y * y + z * z);
    let m01 = 2.0 * (x * y - z * w);
    let m02 = 2.0 * (x * z + y * w);
    let m10 = 2.0 * (x * y + z * w);
    let m11 = 1.0 - 2.0 * (x * x + z * z);
    let m12 = 2.0 * (y * z - x * w);
    let m20 = 2.0 * (x * z - y * w);
    let m21 = 2.0 * (y * z + x * w);
    let m22 = 1.0 - 2.0 * (x * x + y * y);
    Point3::new(
        m00 * point.x + m01 * point.y + m02 * point.z,
        m10 * point.x + m11 * point.y + m12 * point.z,
        m20 * point.x + m21 * point.y + m22 * point.z,
    )
}

fn quantize(value: f64, width: f64) -> i64 {
    (value / width).floor() as i64
}

fn estimate_geometry_memory_mb(nodes: usize, buckets: usize) -> f64 {
    ((nodes * std::mem::size_of::<PatchNode>()) + (buckets * 64)) as f64 / (1024.0 * 1024.0)
}

fn combinations_3(value: usize) -> usize {
    if value < 3 {
        return value;
    }
    value.saturating_mul(value - 1).saturating_mul(value - 2) / 6
}

#[cfg(test)]
mod tests {
    use super::*;
    use gls_types::{InteractionKind, ToleranceProfile, ToleranceProfileLevel};

    #[test]
    fn kabsch_rmsd_recovers_translated_points() {
        let reference = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let mobile = [
            Point3::new(10.0, -4.0, 1.0),
            Point3::new(11.0, -4.0, 1.0),
            Point3::new(10.0, -3.0, 1.0),
        ];

        assert!(kabsch_rmsd(&reference, &mobile).expect("rmsd") < 1e-9);
    }

    #[test]
    fn geometry_hash_guard_triggers_on_bucket_occupancy() {
        let profile = ToleranceProfile {
            profile_id: "p".to_string(),
            dataset_id: "d".to_string(),
            created_at: "t".to_string(),
            interaction_kind: InteractionKind::HydrogenBond,
            ligand_class: "generic".to_string(),
            structure_confidence_bin: "high".to_string(),
            radius_tolerance: 1.0,
            angle_tolerance_deg: 10.0,
            distance_bin_width: 10.0,
            vector_angle_bin_width: 5.0,
            max_bucket_occupancy: 1,
            max_seed_triplets: 10,
            max_memory_mb: 100.0,
            recall_at_profile: 1.0,
            hard_negative_rejection: 1.0,
            hard_negative_false_positive_rate: 0.0,
            bucket_occupancy_p95: 1.0,
            peak_memory_mb: 1.0,
            runtime_ms_per_fixture: 1.0,
            generated_by_fixture_calibration: true,
            profile_level: ToleranceProfileLevel::Standard,
        };
        let node = PatchNode {
            node_id: "n".to_string(),
            residue: 'D',
            kind: InteractionKind::HydrogenBond,
            point: Point3Data::new(0.0, 0.0, 0.0),
            vector: Point3Data::new(1.0, 0.0, 0.0),
            sasa: 0.2,
            pocket_depth: 1.0,
            confidence: 0.9,
        };
        let report = build_geometry_hash(&[node.clone(), node], &profile);

        assert!(report.guard_triggered);
        assert_eq!(
            report.failure_reason.as_deref(),
            Some("geometry_hash_guard_triggered")
        );
    }
}
