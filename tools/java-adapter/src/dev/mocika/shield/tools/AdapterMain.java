package dev.mocika.shield.tools;

import java.io.IOException;

/** 私有适配入口；固定证书语义及两个工具，不开放任意主类或历史证书修改。 */
public final class AdapterMain {
    public static void main(String[] nativeArgs) {
        if (nativeArgs.length != 0) {
            reject("JAVA_ADAPTER_ARGUMENTS_UNSUPPORTED", 2);
            return;
        }
        final String[] request;
        try {
            request = Request.read(System.in);
        } catch (IOException error) {
            reject("JAVA_ADAPTER_REQUEST_INVALID", 2);
            return;
        }
        String version = AdapterMain.class.getPackage().getImplementationVersion();
        if (version == null || !version.matches("[0-9]+\\.[0-9]+\\.[0-9]+[A-Za-z0-9.+-]*")) {
            reject("JAVA_ADAPTER_RESOURCE_INVALID", 3);
            return;
        }
        if (CertificateReader.supports(request)) {
            System.exit(CertificateReader.run(version, request));
            return;
        }
        if (CertificateCreator.supports(request)) {
            System.exit(CertificateCreator.run(version, request));
            return;
        }
        if (JarLauncher.supports(request)) {
            int status = JarLauncher.run(request);
            // 正常返回保留原工具非守护线程；主动退出与异常则维持明确退出状态。
            if (status != 0) {
                System.exit(status);
            }
            return;
        }
        if (request.length != 1 || !request[0].equals("hello")) {
            reject("JAVA_ADAPTER_OPERATION_UNSUPPORTED", 2);
            return;
        }
        System.out.println("{\"protocol_version\":" + Request.PROTOCOL
            + ",\"application_version\":\"" + version + "\",\"operation\":\"hello\"}");
    }

    private static void reject(String code, int status) {
        // 仅固定分类，绝不打印请求或环境内容。
        System.err.println(code);
        System.exit(status);
    }

    private AdapterMain() {}
}
