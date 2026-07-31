#include <jni.h>
#include <stdio.h>
#include <string.h>

static jstring native_round_trip(JNIEnv *env, jclass owner, jint input) {
    (void)owner;
    jclass callback = (*env)->FindClass(
            env, "dev/mocika/shield/smoke/JniDexSeparationCallback");
    if (callback == NULL) {
        return NULL;
    }
    jmethodID value = (*env)->GetStaticMethodID(
            env, callback, "value", "(I)Ljava/lang/String;");
    if (value == NULL) {
        return NULL;
    }
    jstring java_value = (jstring)(*env)->CallStaticObjectMethod(env, callback, value, input + 3);
    if ((*env)->ExceptionCheck(env)) {
        return NULL;
    }
    if (java_value == NULL) {
        return (*env)->NewStringUTF(env, "native-null");
    }

    const char *characters = (*env)->GetStringUTFChars(env, java_value, NULL);
    if (characters == NULL) {
        return NULL;
    }
    char output[96];
    int written = snprintf(output, sizeof(output), "native-%s", characters);
    (*env)->ReleaseStringUTFChars(env, java_value, characters);
    if (written < 0 || (size_t)written >= sizeof(output)) {
        return NULL;
    }
    return (*env)->NewStringUTF(env, output);
}

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *reserved) {
    (void)reserved;
    JNIEnv *env = NULL;
    if ((*vm)->GetEnv(vm, (void **)&env, JNI_VERSION_1_6) != JNI_OK) {
        return JNI_ERR;
    }
    jclass bridge = (*env)->FindClass(
            env, "dev/mocika/shield/smoke/JniDexSeparationBridge");
    if (bridge == NULL) {
        return JNI_ERR;
    }
    JNINativeMethod methods[] = {
            {"nativeRoundTrip", "(I)Ljava/lang/String;", (void *)native_round_trip},
    };
    if ((*env)->RegisterNatives(env, bridge, methods, 1) != JNI_OK) {
        return JNI_ERR;
    }
    return JNI_VERSION_1_6;
}
