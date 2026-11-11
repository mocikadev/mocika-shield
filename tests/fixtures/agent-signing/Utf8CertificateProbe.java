import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.security.Key;
import java.security.KeyStore;
import java.security.MessageDigest;
import java.security.cert.Certificate;
import java.security.cert.X509Certificate;
import java.util.Arrays;
import java.util.Enumeration;
import javax.security.auth.x500.X500Principal;

// 只用合成证书验证标准 Java 8 接口，不生成自定义证书、不迁移真实材料。
final class Utf8CertificateProbe {
    static void run(String[] args) throws Exception {
        if (args.length != 6 || !args[2].equals("中文Release") || !args[3].equals("CN=中文主题")) {
            throw new IllegalArgumentException();
        }
        String expectedPassword = args[4].equals("JKS")
            ? "隔离口令一二三456" : "isolated-path-probe-password";
        char[] password = System.getenv("SHIELD_PATH_PROBE_PASS").toCharArray();
        boolean passwordMatches = expectedPassword.equals(new String(password));
        try {
            KeyStore store = KeyStore.getInstance(args[4]);
            try (InputStream input = Files.newInputStream(Paths.get(args[0]))) {
                store.load(input, password);
            }
            Key key = store.getKey("release", password);
            Certificate[] chain = store.getCertificateChain("release");
            store.deleteEntry("release");
            store.setKeyEntry(args[2], key, password, chain);
            try (OutputStream output = Files.newOutputStream(Paths.get(args[1]),
                    StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE)) {
                store.store(output, password);
            }
            try (InputStream input = Files.newInputStream(Paths.get(args[1]))) {
                store.load(input, password);
            }
            Enumeration<String> aliases = store.aliases();
            boolean aliasMatches = aliases.hasMoreElements()
                && aliases.nextElement().equals("中文release") && !aliases.hasMoreElements();
            X509Certificate certificate = (X509Certificate) store.getCertificate(args[2]);
            certificate.verify(certificate.getPublicKey());
            byte[] digest = MessageDigest.getInstance("SHA-256").digest(certificate.getEncoded());
            StringBuilder fingerprint = new StringBuilder();
            for (byte value : digest) {
                fingerprint.append(String.format(java.util.Locale.ROOT, "%02x", value & 255));
            }
            // 重新读取私钥，证明密码、Alias 与落盘文件不只是内存内匹配。
            boolean keyMatches = Arrays.equals(key.getEncoded(), store.getKey(args[2], password).getEncoded());
            System.out.println("{\"alias_matches\":" + aliasMatches
                + ",\"subject_matches\":" + certificate.getSubjectX500Principal().equals(
                    new X500Principal("CN=中文主题"))
                + ",\"fingerprint_matches\":" + fingerprint.toString().equals(args[5])
                + ",\"password_matches\":" + passwordMatches
                + ",\"key_matches\":" + keyMatches + "}");
        } finally {
            Arrays.fill(password, '\0');
        }
    }
}
