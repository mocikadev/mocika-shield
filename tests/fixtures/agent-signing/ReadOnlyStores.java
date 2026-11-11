package dev.mocika.shield.tools;

import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.security.Key;
import java.security.KeyStore;
import java.security.cert.Certificate;
import java.util.Arrays;
import javax.crypto.spec.SecretKeySpec;

/** 仅生成本次测试材料；不打入正式 JAR，不操作真实证书。 */
public final class ReadOnlyStores {
    public static void main(String[] ignored) throws Exception {
        String[] request = Request.read(System.in);
        if (request.length != 5 || !request[0].equals("fixture-create")) {
            throw new IllegalArgumentException();
        }
        char[] initial = System.getenv("SHIELD_READONLY_BOOTSTRAP_PASS").toCharArray();
        char[] password = System.getenv("SHIELD_READONLY_FIXTURE_PASS").toCharArray();
        try {
            KeyStore source = KeyStore.getInstance("JKS");
            try (InputStream input = Files.newInputStream(Paths.get(request[1]))) {
                source.load(input, initial);
            }
            Key key = source.getKey("release", initial);
            Certificate[] chain = source.getCertificateChain("release");
            KeyStore target = KeyStore.getInstance(request[3]);
            target.load(null, password);
            if (request[4].equals("normal")) {
                // JKS 故意分离 Key 密码，证明只读证书查询不需要恢复私钥。
                char[] keyPassword = request[3].equals("JKS") ? "separate-key-password".toCharArray() : password.clone();
                try {
                    target.setKeyEntry("中文Release", key, keyPassword, chain);
                } finally {
                    Arrays.fill(keyPassword, '\0');
                }
                target.setKeyEntry("含,引号\"😀", key, password, chain);
                target.setCertificateEntry("受信 任", chain[0]);
                if (request[3].equals("PKCS12")) {
                    target.setEntry("secret", new KeyStore.SecretKeyEntry(new SecretKeySpec(new byte[16], "AES")),
                        new KeyStore.PasswordProtection(password));
                }
            } else if (request[4].equals("many")) {
                for (int i = 0; i < 257; i++) {
                    target.setCertificateEntry("entry-" + i, chain[0]);
                }
            } else if (request[4].equals("long")) {
                target.setCertificateEntry(new String(new char[4097]).replace('\0', 'x'), chain[0]);
            } else if (request[4].equals("wide")) {
                // 单项和条数均在预算内，仅完整 JSON 转义结果超限。
                for (int i = 0; i < 64; i++) {
                    target.setCertificateEntry(new String(new char[200]).replace('\0', '中') + i, chain[0]);
                }
            } else if (!request[4].equals("empty")) {
                throw new IllegalArgumentException();
            }
            try (OutputStream output = Files.newOutputStream(Paths.get(request[2]), StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)) {
                target.store(output, password);
            }
            System.out.print("{\"created\":true}");
        } finally {
            Arrays.fill(initial, '\0');
            Arrays.fill(password, '\0');
        }
    }
}
