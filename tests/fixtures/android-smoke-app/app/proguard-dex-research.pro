# 保留被反射选择的业务样本身份；其余测试组件仍由 R8 正常处理。
-keep class dev.mocika.shield.smoke.DexSeparationCases { public static *; }

# Kotlin 样本及其编译器生成类是本批次被选择的研究输入。
-keep class dev.mocika.shield.smoke.KotlinDexSeparationCases { *; }
-keep class dev.mocika.shield.smoke.KotlinDexSeparationCases$* { *; }

# 多 DEX 次包通过类名回调主 DEX，主包样本身份必须稳定。
-keep class dev.mocika.shield.smoke.MultiDexMainCases { public static *; }

# 保持 ARouter 运行期扫描所需的生成类和研究目标类身份。
-keep public class com.alibaba.android.arouter.routes.** { *; }
-keep public class com.alibaba.android.arouter.facade.** { *; }
-keep class * implements com.alibaba.android.arouter.facade.template.ISyringe { *; }
-keep class * implements com.alibaba.android.arouter.facade.template.IProvider { *; }
-keep class dev.mocika.shield.smoke.ARouterDexSeparationTarget { public static *; }
-keep class dev.mocika.shield.smoke.ARouterDexSeparationReporter { public static *; }
-dontwarn javax.lang.model.element.Element

# JNI 动态注册和 Native 回调依赖稳定的类、方法身份，必须全量保留。
-keep class dev.mocika.shield.smoke.JniDexSeparationBridge { *; }
-keep class dev.mocika.shield.smoke.JniDexSeparationCallback { *; }
-keep class dev.mocika.shield.smoke.JniDexSeparationReporter { public static *; }
