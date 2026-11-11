import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.Key;
import java.security.KeyStore;
import java.security.cert.Certificate;
import java.security.cert.X509Certificate;
import java.util.Arrays;
import java.util.Enumeration;
import java.util.Locale;
import javax.security.auth.x500.X500Principal;

// 仅测试标准证书接口；期望值从 UTF-8 文件读取，只输出固定布尔值。
public final class CertificateContentProbe {
    public static void main(String[] args) throws Exception {
        String expectedAlias = read(args[1]).toLowerCase(Locale.ROOT);
        String expectedSubject = read(args[2]);
        char[] password = System.getenv("SHIELD_PATH_PROBE_PASS").toCharArray();
        try {
            KeyStore store = KeyStore.getInstance("PKCS12");
            try (InputStream input = Files.newInputStream(Paths.get(args[0]))) {
                store.load(input, password);
            }
            Enumeration<String> aliases = store.aliases();
            if (!aliases.hasMoreElements()) {
                throw new IllegalStateException("自有夹具必须包含一个别名");
            }
            String actualAlias = aliases.nextElement();
            if (aliases.hasMoreElements()) {
                throw new IllegalStateException("自有夹具不得包含多个别名");
            }
            X509Certificate certificate = (X509Certificate) store.getCertificate(actualAlias);
            if (args.length == 4) {
                Key key = store.getKey(actualAlias, password);
                Certificate[] chain = store.getCertificateChain(actualAlias);
                store.deleteEntry(actualAlias);
                store.setKeyEntry(expectedAlias, key, password, chain);
                Path target = Paths.get(args[3]);
                try (OutputStream output = Files.newOutputStream(target,
                        java.nio.file.StandardOpenOption.CREATE_NEW,
                        java.nio.file.StandardOpenOption.WRITE)) {
                    store.store(output, password);
                }
                // 重新从实际文件读取，而不是以内存中的 setKeyEntry 成功作为证据。
                try (InputStream input = Files.newInputStream(target)) {
                    store.load(input, password);
                }
                aliases = store.aliases();
                actualAlias = aliases.nextElement();
                if (aliases.hasMoreElements()) {
                    throw new IllegalStateException("独立夹具不得包含多个别名");
                }
                certificate = (X509Certificate) store.getCertificate(actualAlias);
            }
            String subject = certificate.getSubjectX500Principal().getName();
            System.out.println("{\"alias_matches\":" + actualAlias.equals(expectedAlias)
                + ",\"subject_matches\":" + certificate.getSubjectX500Principal().equals(
                    new X500Principal(expectedSubject))
                + ",\"alias_has_question_mark\":" + (actualAlias.indexOf('?') >= 0)
                + ",\"subject_has_question_mark\":" + (subject.indexOf('?') >= 0) + "}");
        } finally {
            Arrays.fill(password, '\0');
        }
    }

    private static String read(String file) throws Exception {
        return new String(Files.readAllBytes(Paths.get(file)), StandardCharsets.UTF_8);
    }
}
