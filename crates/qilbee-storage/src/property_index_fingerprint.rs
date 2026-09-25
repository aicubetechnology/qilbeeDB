//! Independent canonical-fnv1a64-v1 reconstruction for offline verification.
//! Deliberately separate from the production property-index hash implementation.
use qilbee_core::PropertyValue;
pub fn hash_property_value(value: &PropertyValue) -> u64 {
    fn bytes(h: &mut u64, data: &[u8]) {
        for byte in data {
            *h = (*h ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    fn number(h: &mut u64, n: u64) {
        bytes(h, &n.to_be_bytes());
    }
    fn sized(h: &mut u64, data: &[u8]) {
        number(h, data.len() as u64);
        bytes(h, data);
    }
    fn float(h: &mut u64, n: f64) {
        number(h, if n == 0.0 { 0 } else { n.to_bits() });
    }
    fn visit(h: &mut u64, v: &PropertyValue) {
        use PropertyValue::*;
        let tag = match v {
            Null => 0,
            Boolean(_) => 1,
            Integer(_) => 2,
            Float(_) => 3,
            String(_) => 4,
            Array(_) => 5,
            Map(_) => 6,
            Bytes(_) => 7,
            Date(_) => 8,
            Time(_) => 9,
            DateTime(_) => 10,
            Duration(_) => 11,
            Point2D { .. } => 12,
            Point3D { .. } => 13,
        };
        bytes(h, &[tag]);
        match v {
            Null => {}
            Boolean(b) => bytes(h, &[u8::from(*b)]),
            Integer(n) => number(h, *n as u64),
            Float(n) => float(h, *n),
            String(s) => sized(h, s.as_bytes()),
            Bytes(b) => sized(h, b),
            Array(a) => {
                number(h, a.len() as u64);
                for v in a {
                    visit(h, v);
                }
            }
            Map(m) => {
                number(h, m.len() as u64);
                let mut entries = m.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
                for (k, v) in entries {
                    sized(h, k.as_bytes());
                    visit(h, v);
                }
            }
            Date(n) => bytes(h, &n.to_be_bytes()),
            Time(n) | DateTime(n) | Duration(n) => number(h, *n as u64),
            Point2D { x, y, srid } => {
                float(h, *x);
                float(h, *y);
                bytes(h, &srid.to_be_bytes());
            }
            Point3D { x, y, z, srid } => {
                float(h, *x);
                float(h, *y);
                float(h, *z);
                bytes(h, &srid.to_be_bytes());
            }
        }
    }
    let mut h = 0xcbf29ce484222325;
    visit(&mut h, value);
    h
}
