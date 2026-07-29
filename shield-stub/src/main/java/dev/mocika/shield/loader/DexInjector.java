package dev.mocika.shield.loader;

import android.content.Context;
import android.os.Build;

import dalvik.system.DexClassLoader;

import java.io.File;
import java.io.IOException;
import java.lang.reflect.Array;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;

/** 按 Android 版本将解密 DEX 注入应用原有的 PathClassLoader。 */
final class DexInjector {

    private DexInjector() {}

    static void inject(Context context, List<File> dexFiles) throws Exception {
        SharedLibraryCompat.prepare(context, dexFiles);
        ClassLoader classLoader = context.getClassLoader();
        File optimizedDirectory = Build.VERSION.SDK_INT < 26
                ? context.getDir("dex_opt", Context.MODE_PRIVATE) : null;

        if (Build.VERSION.SDK_INT >= 24) {
            injectWithAddDexPath(classLoader, dexFiles, optimizedDirectory);
            return;
        }
        if (Build.VERSION.SDK_INT >= 21) {
            injectWithElementFactory(classLoader, dexFiles, optimizedDirectory,
                    factoryMethodNames(Build.VERSION.SDK_INT));
            return;
        }
        if (Build.VERSION.SDK_INT >= 19) {
            injectWithDalvikClassLoader(classLoader, dexFiles, optimizedDirectory);
            return;
        }
        throw new UnsupportedOperationException("D01");
    }

    /**
     * Dalvik 没有 ART 后续版本的 Element 工厂入口。让系统 DexClassLoader 完成
     * DexFile.loadDex 与 Element 构造，再把生成的元素前插到应用 PathClassLoader。
     */
    private static void injectWithDalvikClassLoader(ClassLoader classLoader, List<File> dexFiles,
                                                     File optimizedDirectory) throws Exception {
        if (optimizedDirectory == null) {
            throw new IOException("D02");
        }
        StringBuilder dexPath = new StringBuilder();
        for (File dexFile : dexFiles) {
            if (dexPath.length() > 0) {
                dexPath.append(File.pathSeparatorChar);
            }
            dexPath.append(dexFile.getAbsolutePath());
        }

        DexClassLoader source = new DexClassLoader(
                dexPath.toString(), optimizedDirectory.getAbsolutePath(), null, classLoader);
        Object sourcePathList = findField(source.getClass(), "pathList").get(source);
        Field sourceElementsField = findField(sourcePathList.getClass(), "dexElements");
        Object[] injected = (Object[]) sourceElementsField.get(sourcePathList);
        if (injected == null || injected.length != dexFiles.size()) {
            throw new IOException("D03");
        }

        Object targetPathList = findField(classLoader.getClass(), "pathList").get(classLoader);
        Field targetElementsField = findField(targetPathList.getClass(), "dexElements");
        prependElements(targetPathList, targetElementsField, injected);
    }

    private static void injectWithAddDexPath(ClassLoader classLoader, List<File> dexFiles,
                                              File optimizedDirectory) throws Exception {
        String[] dexPaths = new String[dexFiles.size()];
        for (int i = 0; i < dexFiles.size(); i++) {
            dexPaths[i] = dexFiles.get(i).getAbsolutePath();
        }
        String optimizedPath = optimizedDirectory == null ? "" : optimizedDirectory.getAbsolutePath();
        if (Ld.p(classLoader, dexPaths, optimizedPath)) {
            return;
        }

        Object pathList = findField(classLoader.getClass(), "pathList").get(classLoader);
        Field elementsField = findField(pathList.getClass(), "dexElements");
        int oldLength = ((Object[]) elementsField.get(pathList)).length;
        Method addDexPath = findMethod(pathList.getClass(), "addDexPath", String.class, File.class);
        for (File dexFile : dexFiles) {
            addDexPath.invoke(pathList, dexFile.getAbsolutePath(), optimizedDirectory);
        }
        moveAppendedElementsToFront(pathList, elementsField, oldLength);
    }

    private static void injectWithElementFactory(ClassLoader classLoader, List<File> dexFiles,
                                                  File optimizedDirectory,
                                                  String[] methodNames) throws Exception {
        Object pathList = findField(classLoader.getClass(), "pathList").get(classLoader);
        Field elementsField = findField(pathList.getClass(), "dexElements");
        ArrayList<File> files = new ArrayList<>(dexFiles);
        ArrayList<IOException> suppressed = new ArrayList<>();
        Method factory = findElementFactory(pathList.getClass(), methodNames);

        Object[] injected;
        try {
            injected = (Object[]) factory.invoke(pathList, files, optimizedDirectory, suppressed);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause();
            if (cause instanceof Exception) throw (Exception) cause;
            throw error;
        }
        if (!suppressed.isEmpty()) {
            mergeSuppressedExceptions(pathList, suppressed);
            IOException error = new IOException("D04");
            error.initCause(suppressed.get(0));
            throw error;
        }
        prependElements(pathList, elementsField, injected);
    }

    static String[] factoryMethodNames(int sdkInt) {
        return sdkInt >= 23
                ? new String[]{"makePathElements", "makeDexElements"}
                : new String[]{"makeDexElements", "makePathElements"};
    }

    static Object[] prepend(Object[] original, Object[] injected) {
        Object[] combined = (Object[]) Array.newInstance(
                original.getClass().getComponentType(), original.length + injected.length);
        System.arraycopy(injected, 0, combined, 0, injected.length);
        System.arraycopy(original, 0, combined, injected.length, original.length);
        return combined;
    }

    private static void prependElements(Object pathList, Field elementsField, Object[] injected)
            throws IllegalAccessException {
        Object[] original = (Object[]) elementsField.get(pathList);
        elementsField.set(pathList, prepend(original, injected));
    }

    private static void moveAppendedElementsToFront(Object pathList, Field elementsField,
                                                     int oldLength) throws IllegalAccessException {
        Object[] elements = (Object[]) elementsField.get(pathList);
        int injectedCount = elements.length - oldLength;
        if (injectedCount <= 0) return;
        Object[] original = new Object[oldLength];
        Object[] injected = new Object[injectedCount];
        System.arraycopy(elements, 0, original, 0, oldLength);
        System.arraycopy(elements, oldLength, injected, 0, injectedCount);
        elementsField.set(pathList, prepend(elementsOfSameType(elements, original),
                elementsOfSameType(elements, injected)));
    }

    private static Object[] elementsOfSameType(Object[] template, Object[] values) {
        Object[] result = (Object[]) Array.newInstance(
                template.getClass().getComponentType(), values.length);
        System.arraycopy(values, 0, result, 0, values.length);
        return result;
    }

    private static Method findElementFactory(Class<?> type, String[] names)
            throws NoSuchMethodException {
        for (String name : names) {
            for (Class<?> current = type; current != null; current = current.getSuperclass()) {
                for (Method method : current.getDeclaredMethods()) {
                    Class<?>[] parameters = method.getParameterTypes();
                    if (method.getName().equals(name)
                            && parameters.length == 3
                            && parameters[0].isAssignableFrom(ArrayList.class)
                            && parameters[1] == File.class
                            && parameters[2].isAssignableFrom(ArrayList.class)) {
                        method.setAccessible(true);
                        return method;
                    }
                }
            }
        }
        throw new NoSuchMethodException(names[0]);
    }

    private static Method findMethod(Class<?> type, String name, Class<?>... parameters)
            throws NoSuchMethodException {
        for (Class<?> current = type; current != null; current = current.getSuperclass()) {
            try {
                Method method = current.getDeclaredMethod(name, parameters);
                method.setAccessible(true);
                return method;
            } catch (NoSuchMethodException ignored) {}
        }
        throw new NoSuchMethodException(name);
    }

    private static Field findField(Class<?> type, String name) throws NoSuchFieldException {
        for (Class<?> current = type; current != null; current = current.getSuperclass()) {
            try {
                Field field = current.getDeclaredField(name);
                field.setAccessible(true);
                return field;
            } catch (NoSuchFieldException ignored) {}
        }
        throw new NoSuchFieldException(name);
    }

    private static void mergeSuppressedExceptions(Object pathList, List<IOException> added)
            throws ReflectiveOperationException {
        Field field = findField(pathList.getClass(), "dexElementsSuppressedExceptions");
        IOException[] existing = (IOException[]) field.get(pathList);
        int existingLength = existing == null ? 0 : existing.length;
        IOException[] combined = new IOException[added.size() + existingLength];
        added.toArray(combined);
        if (existing != null) {
            System.arraycopy(existing, 0, combined, added.size(), existingLength);
        }
        field.set(pathList, combined);
    }
}
