use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::dex_classes::defined_classes;

const AROUTER_ROUTES_PREFIX: &str = "com/alibaba/android/arouter/routes/";
const AROUTER_REGISTRY_PREFIXES: [&str; 3] = [
    "ARouter$$Root$$",
    "ARouter$$Providers$$",
    "ARouter$$Interceptors$$",
];

/// 扫描 DEX 目录，提取所有 ARouter 路由表类的全限定名。
/// 解析 DEX header 的 class_defs 段，不依赖任何运行时 API。
pub fn scan_arouter_routes(dex_dir: &Path) -> Result<Vec<String>> {
    let mut routes = Vec::new();

    let mut dex_paths: Vec<_> = fs::read_dir(dex_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dex"))
        .collect();
    dex_paths.sort();

    for path in &dex_paths {
        let data = fs::read(path)?;
        for name in defined_classes(&data)? {
            if is_arouter_registry_class(&name) {
                let java_name = name.replace('/', ".");
                routes.push(java_name);
            }
        }
    }

    routes.sort();
    routes.dedup();
    Ok(routes)
}

fn is_arouter_registry_class(name: &str) -> bool {
    let Some(simple_name) = name.strip_prefix(AROUTER_ROUTES_PREFIX) else {
        return false;
    };
    AROUTER_REGISTRY_PREFIXES.iter().any(|prefix| {
        simple_name
            .strip_prefix(prefix)
            .is_some_and(|module_name| !module_name.is_empty() && !module_name.contains('$'))
    })
}

#[cfg(test)]
mod tests {
    use super::{is_arouter_registry_class, scan_arouter_routes};
    use crate::dex_classes::fixture;
    use std::fs;

    #[test]
    fn 共享严格读取后仍只输出可注册路由入口() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("classes.dex"),
            fixture("com/alibaba/android/arouter/routes/ARouter$$Root$$app"),
        )
        .unwrap();
        fs::write(
            temp.path().join("classes2.dex"),
            fixture("com/alibaba/android/arouter/routes/ARouter$$Group$$home"),
        )
        .unwrap();
        fs::write(temp.path().join("classes3.dex"), fixture("example/业务类")).unwrap();
        assert_eq!(
            scan_arouter_routes(temp.path()).unwrap(),
            vec!["com.alibaba.android.arouter.routes.ARouter$$Root$$app"]
        );
    }

    #[test]
    fn 只接受_arouter_可注册入口类() {
        assert!(is_arouter_registry_class(
            "com/alibaba/android/arouter/routes/ARouter$$Root$$featurehome"
        ));
        assert!(is_arouter_registry_class(
            "com/alibaba/android/arouter/routes/ARouter$$Providers$$arouterapi"
        ));
        assert!(is_arouter_registry_class(
            "com/alibaba/android/arouter/routes/ARouter$$Interceptors$$app"
        ));
        assert!(!is_arouter_registry_class(
            "com/alibaba/android/arouter/routes/ARouter$$Group$$home"
        ));
        assert!(!is_arouter_registry_class(
            "com/alibaba/android/arouter/routes/ARouter$$Root$$featurehome$1"
        ));
    }
}
