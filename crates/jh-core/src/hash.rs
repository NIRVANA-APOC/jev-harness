//! 内置的稳定非加密内容摘要哈希。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 内容摘要哈希。
///
/// **非加密用途**：只用于变更检测与审计标识——给一条证据一个稳定的短标识、比较两次记录的内容是否
/// 相同。它没有抗碰撞性质，不能用于防篡改、签名或任何安全判定（取值可以被刻意构造成碰撞）。
///
/// 算法取 FNV-1a 64 位：只用整数运算实现，跨进程、跨平台、跨编译器版本取值恒定。标准库的
/// `std::collections::hash_map::DefaultHasher` 不保证跨版本稳定，因此不能拿它当审计标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub u64);

impl ContentHash {
    /// 对字节串求摘要。
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(hash)
    }

    /// 对 JSON 内容求摘要。
    ///
    /// 取 `serde_json` 对 [`serde_json::Value`] 的默认编码（对象键按有序映射写出），因此同一份内容
    /// 无论以什么顺序构造，摘要都一样。
    pub fn of_json(content: &serde_json::Value) -> Self {
        Self::of_bytes(&serde_json::to_vec(content).expect("JSON 值总能序列化"))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fnv1a64:{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn fnv1a64_reproduces_the_published_vectors() {
        // 公开规范里的 FNV-1a 64 位测试向量，用来钉住「稳定」这个性质。
        assert_eq!(
            ContentHash::of_bytes(b""),
            ContentHash(0xcbf2_9ce4_8422_2325)
        );
        assert_eq!(
            ContentHash::of_bytes(b"a"),
            ContentHash(0xaf63_dc4c_8601_ec8c)
        );
        assert_eq!(
            ContentHash::of_bytes(b"foobar"),
            ContentHash(0x8594_4171_f739_67e8)
        );
    }

    #[test]
    fn content_hash_is_the_same_for_equivalent_json_and_different_for_changed_json() {
        let left = json!({"cache": "on", "retries": 3});
        let right = json!({"retries": 3, "cache": "on"});

        assert_eq!(ContentHash::of_json(&left), ContentHash::of_json(&right));
        assert_ne!(
            ContentHash::of_json(&left),
            ContentHash::of_json(&json!({"cache": "off", "retries": 3}))
        );
    }

    #[test]
    fn content_hash_is_a_plain_number_on_the_wire_and_hex_when_printed() {
        let hash = ContentHash::of_bytes(b"");
        assert_eq!(
            serde_json::to_value(hash).unwrap(),
            json!(0xcbf2_9ce4_8422_2325u64)
        );
        assert_eq!(hash.to_string(), "fnv1a64:cbf29ce484222325");
    }
}
