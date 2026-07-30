#include <jni.h>

extern "C" JNIEXPORT jstring JNICALL
Java_dev_mocika_shield_memorypayload_PayloadActivity_nativeMarker(JNIEnv* env, jclass) {
    return env->NewStringUTF("NATIVE_OK");
}
