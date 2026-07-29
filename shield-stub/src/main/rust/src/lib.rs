mod anti_debug;
mod bin_loader;
mod crypto;
mod root_environment;

use std::ffi::c_void;

use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jobjectArray, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use jni::NativeMethod;

/// 动态注册 native 方法，使 Rust 函数名不出现在 .dynsym，切断符号可读性。
#[no_mangle]
pub extern "C" fn JNI_OnLoad(vm: *mut jni::sys::JavaVM, _reserved: *mut c_void) -> jni::sys::jint {
    let vm = match unsafe { jni::JavaVM::from_raw(vm) } {
        Ok(v) => v,
        Err(_) => return jni::sys::JNI_ERR,
    };
    let mut env = match vm.get_env() {
        Ok(e) => e,
        Err(_) => return jni::sys::JNI_ERR,
    };
    let class = match env.find_class(env!("STUB_BINLOADER_CLASS")) {
        Ok(c) => c,
        Err(_) => return jni::sys::JNI_ERR,
    };
    let methods = [
        NativeMethod {
            name: env!("STUB_METHOD_INJECT_DEX").into(),
            sig: "(Ljava/lang/ClassLoader;[Ljava/lang/String;Ljava/lang/String;)Z".into(),
            fn_ptr: f1 as *mut c_void,
        },
        NativeMethod {
            name: env!("STUB_METHOD_EXTRACT_DECRYPT").into(),
            sig: "(Landroid/content/Context;[B)[[B".into(),
            fn_ptr: f2 as *mut c_void,
        },
        NativeMethod {
            name: env!("STUB_METHOD_CHECK_ENV").into(),
            sig: "()I".into(),
            fn_ptr: f3 as *mut c_void,
        },
    ];
    match env.register_native_methods(&class, &methods) {
        Ok(_) => jni::sys::JNI_VERSION_1_6,
        Err(_) => jni::sys::JNI_ERR,
    }
}

/// 每次进程启动的环境检查入口。位 0 表示反调试命中，位 1 表示高置信 Root 命中。
extern "C" fn f3(_env: JNIEnv, _class: JClass) -> jint {
    let mut signals = 0;
    if anti_debug::check() {
        signals |= 1;
    }
    if root_environment::check() {
        signals |= 2;
    }
    signals
}

/// DEX 注入：通过 JNI 将解密后的 DEX 插入 PathClassLoader，使 app 类优先加载。
/// 成功返回 JNI_TRUE，失败返回 JNI_FALSE（Java 层降级到反射方案）。
extern "C" fn f1(
    mut env: JNIEnv,
    _class: JClass,
    class_loader: JObject,
    dex_paths: jobjectArray,
    opt_dir_path: JString,
) -> jboolean {
    match f1_impl(&mut env, class_loader, dex_paths, opt_dir_path) {
        Ok(()) => JNI_TRUE,
        Err(e) => {
            android_log_warn(&mut env, &format!("注入失败，降级到 Java 反射：{}", e));
            JNI_FALSE
        }
    }
}

fn f1_impl(
    env: &mut JNIEnv,
    class_loader: JObject,
    dex_paths: jobjectArray,
    opt_dir_path: JString,
) -> Result<(), String> {
    let dex_paths_jarr = unsafe { JObjectArray::from_raw(dex_paths) };

    let base_dcl_class = env
        .find_class("dalvik/system/BaseDexClassLoader")
        .map_err(|e| format!("FindClass BaseDexClassLoader: {}", e))?;
    let path_list_field = env
        .get_field_id(&base_dcl_class, "pathList", "Ldalvik/system/DexPathList;")
        .map_err(|e| format!("GetFieldID pathList: {}", e))?;
    let path_list = env
        .get_field_unchecked(
            &class_loader,
            path_list_field,
            jni::signature::ReturnType::Object,
        )
        .map_err(|e| format!("GetObjectField pathList: {}", e))?
        .l()
        .map_err(|e| format!("pathList 不是对象: {}", e))?;

    let dex_path_list_class = env
        .find_class("dalvik/system/DexPathList")
        .map_err(|e| format!("FindClass DexPathList: {}", e))?;
    let dex_elements_field = env
        .get_field_id(
            &dex_path_list_class,
            "dexElements",
            "[Ldalvik/system/DexPathList$Element;",
        )
        .map_err(|e| format!("GetFieldID dexElements: {}", e))?;

    let old_elements_obj = env
        .get_field_unchecked(
            &path_list,
            dex_elements_field,
            jni::signature::ReturnType::Array,
        )
        .map_err(|e| format!("GetObjectField dexElements(before): {}", e))?
        .l()
        .map_err(|e| format!("dexElements 不是对象: {}", e))?;
    let old_len = env
        .get_array_length(unsafe { &JObjectArray::from_raw(old_elements_obj.as_raw()) })
        .map_err(|e| format!("GetArrayLength dexElements: {}", e))? as usize;

    let opt_dir_str: String = env
        .get_string(&opt_dir_path)
        .map_err(|e| format!("GetString optDirPath: {}", e))?
        .into();
    let opt_dir_obj: JObject = if opt_dir_str.is_empty() {
        JObject::null()
    } else {
        let file_class = env
            .find_class("java/io/File")
            .map_err(|e| format!("FindClass File: {}", e))?;
        let path_jstr = env
            .new_string(&opt_dir_str)
            .map_err(|e| format!("NewString optDirPath: {}", e))?;
        env.new_object(
            &file_class,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&path_jstr)],
        )
        .map_err(|e| format!("new File(optDirPath): {}", e))?
    };

    let path_count = env
        .get_array_length(&dex_paths_jarr)
        .map_err(|e| format!("GetArrayLength dexPaths: {}", e))?;
    for i in 0..path_count {
        let path_obj = env
            .get_object_array_element(&dex_paths_jarr, i)
            .map_err(|e| format!("GetObjectArrayElement dexPaths[{}]: {}", i, e))?;
        env.call_method(
            &path_list,
            "addDexPath",
            "(Ljava/lang/String;Ljava/io/File;)V",
            &[JValue::Object(&path_obj), JValue::Object(&opt_dir_obj)],
        )
        .map_err(|e| format!("CallMethod addDexPath[{}]: {}", i, e))?;
        if env.exception_check().unwrap_or(false) {
            env.exception_clear().ok();
            return Err(format!("addDexPath[{}] 抛出 Java 异常", i));
        }
    }

    let new_elements_obj = env
        .get_field_unchecked(
            &path_list,
            dex_elements_field,
            jni::signature::ReturnType::Array,
        )
        .map_err(|e| format!("GetObjectField dexElements(after): {}", e))?
        .l()
        .map_err(|e| format!("dexElements(after) 不是对象: {}", e))?;
    let new_elements_jarr = unsafe { JObjectArray::from_raw(new_elements_obj.as_raw()) };
    let new_len =
        env.get_array_length(&new_elements_jarr)
            .map_err(|e| format!("GetArrayLength dexElements(after): {}", e))? as usize;
    let injected = new_len.saturating_sub(old_len);

    if injected > 0 {
        let element_class = env
            .find_class("dalvik/system/DexPathList$Element")
            .map_err(|e| format!("FindClass Element: {}", e))?;
        let reordered = env
            .new_object_array(new_len as i32, &element_class, JObject::null())
            .map_err(|e| format!("NewObjectArray reordered: {}", e))?;
        let reordered_jarr = unsafe { JObjectArray::from_raw(reordered.as_raw()) };

        for i in 0..injected {
            let elem = env
                .get_object_array_element(&new_elements_jarr, (old_len + i) as i32)
                .map_err(|e| format!("GetElement new[{}]: {}", old_len + i, e))?;
            env.set_object_array_element(&reordered_jarr, i as i32, &elem)
                .map_err(|e| format!("SetElement reordered[{}]: {}", i, e))?;
        }
        for i in 0..old_len {
            let elem = env
                .get_object_array_element(&new_elements_jarr, i as i32)
                .map_err(|e| format!("GetElement old[{}]: {}", i, e))?;
            env.set_object_array_element(&reordered_jarr, (injected + i) as i32, &elem)
                .map_err(|e| format!("SetElement reordered[{}]: {}", injected + i, e))?;
        }

        env.set_field_unchecked(&path_list, dex_elements_field, JValue::Object(&reordered))
            .map_err(|e| format!("SetObjectField dexElements: {}", e))?;
    }

    Ok(())
}

/// 通过 JNI 获取当前 APK 证书的 SHA-256 指纹（大写 hex，64 字符）。
/// ctx 由调用方（attachBaseContext 阶段）直接传入，规避 ActivityThread.currentApplication()
/// 在 Application 初始化阶段返回 null 的问题。
fn jni_get_actual_signature(env: &mut JNIEnv, ctx: &JObject) -> Result<String, String> {
    let bin_loader_class = env
        .find_class(env!("STUB_BINLOADER_CLASS"))
        .map_err(|e| format!("e1: {}", e))?; // FindClass 失败
    let sig_jstr = env
        .call_static_method(
            &bin_loader_class,
            env!("STUB_METHOD_GET_SIG"),
            "(Landroid/content/Context;)Ljava/lang/String;",
            &[JValue::Object(ctx)],
        )
        .map_err(|e| format!("e2: {}", e))? // 调用签名获取方法失败
        .l()
        .map_err(|e| format!("e2t: {}", e))?; // 返回值类型错误
    if sig_jstr.is_null() {
        return Err("e2n".to_string()); // 签名获取返回 null
    }

    let sig_jstring = unsafe { JString::from_raw(sig_jstr.into_raw()) };
    let sig: String = env
        .get_string(&sig_jstring)
        .map_err(|e| format!("GetString signature: {}", e))?
        .into();
    Ok(sig)
}

fn timing_safe_eq(a: &[u8], b: &[u8]) -> bool {
    let len_ok = a.len() == b.len();
    let cmp_len = a.len().max(b.len());
    let mut diff: u8 = if len_ok { 0 } else { 1 };
    for i in 0..cmp_len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

fn android_log_warn(env: &mut JNIEnv, msg: &str) {
    android_log(env, "w", msg);
}

fn android_log(env: &mut JNIEnv, method: &str, msg: &str) {
    let Ok(log_cls) = env.find_class("android/util/Log") else {
        return;
    };
    let Ok(tag) = env.new_string("ax") else {
        return;
    };
    let Ok(jmsg) = env.new_string(msg) else {
        return;
    };
    let _ = env.call_static_method(
        &log_cls,
        method,
        "(Ljava/lang/String;Ljava/lang/String;)I",
        &[JValue::Object(&tag), JValue::Object(&jmsg)],
    );
}

/// DEX 解密：从 classes.dex 末尾提取 MSHD payload，解密解压后返回各 DEX 字节数组。
/// ctx 用于在 Native 层获取当前证书指纹，参与密钥派生与签名校验。
extern "C" fn f2(
    mut env: JNIEnv,
    _class: JClass,
    ctx: JObject,
    dex_data: JByteArray,
) -> jobjectArray {
    // 反调试检测：先于所有解密动作，检测到调试器或 Frida 注入则立即拒绝。
    if anti_debug::check() {
        android_log_warn(&mut env, "dbg");
        let _ = env.throw_new("java/lang/RuntimeException", "dbg");
        return std::ptr::null_mut();
    }
    let dex_bytes = match env.convert_byte_array(&dex_data) {
        Ok(b) => b,
        Err(e) => {
            let _ = env.throw_new(
                "java/lang/RuntimeException",
                format!("读取 dex_data 失败: {}", e),
            );
            return std::ptr::null_mut();
        }
    };

    let payload = match bin_loader::extract_from_dex_tail(&dex_bytes) {
        Ok(p) => p,
        Err(e) => {
            let _ = env.throw_new(
                "java/lang/RuntimeException",
                format!("提取 MSHD payload 失败: {}", e),
            );
            return std::ptr::null_mut();
        }
    };

    let actual_signature = match jni_get_actual_signature(&mut env, &ctx) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new(
                "java/lang/RuntimeException",
                format!("e3: {}", e), // 无法获取当前签名
            );
            return std::ptr::null_mut();
        }
    };

    let (expected_signature, dex_files) =
        match bin_loader::parse_and_decompress_all(payload, actual_signature.as_bytes()) {
            Ok(pair) => pair,
            Err(e) => {
                let _ = env.throw_new("java/lang/RuntimeException", format!("解密解压失败: {}", e));
                return std::ptr::null_mut();
            }
        };

    if !expected_signature.is_empty() {
        if !timing_safe_eq(actual_signature.as_bytes(), expected_signature.as_bytes()) {
            android_log_warn(&mut env, "sg");
            let _ = env.throw_new(
                "java/lang/RuntimeException",
                "e4", // 签名不匹配，拒绝启动
            );
            return std::ptr::null_mut();
        }
    }

    let byte_array_class = match env.find_class("[B") {
        Ok(cls) => cls,
        Err(e) => {
            let _ = env.throw_new(
                "java/lang/RuntimeException",
                format!("查找 byte[] 类失败: {}", e),
            );
            return std::ptr::null_mut();
        }
    };

    let dex_count = match i32::try_from(dex_files.len()) {
        Ok(n) => n,
        Err(_) => {
            let _ = env.throw_new("java/lang/RuntimeException", "DEX 数量超过 i32 上限");
            return std::ptr::null_mut();
        }
    };
    let result_array = match env.new_object_array(dex_count, &byte_array_class, JObject::null()) {
        Ok(arr) => arr,
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", format!("创建数组失败: {}", e));
            return std::ptr::null_mut();
        }
    };

    for (i, dex_data) in dex_files.iter().enumerate() {
        let byte_array = match env.byte_array_from_slice(dex_data) {
            Ok(arr) => arr,
            Err(e) => {
                let _ = env.throw_new(
                    "java/lang/RuntimeException",
                    format!("创建 byte[] 失败: {}", e),
                );
                return std::ptr::null_mut();
            }
        };
        if let Err(e) = env.set_object_array_element(&result_array, i as i32, byte_array) {
            let _ = env.throw_new(
                "java/lang/RuntimeException",
                format!("设置数组元素失败: {}", e),
            );
            return std::ptr::null_mut();
        }
    }

    result_array.into_raw()
}
