package dev.mocika.shield.tools;

import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.UnrecoverableKeyException;
import java.security.cert.X509Certificate;
import java.util.Arrays;
import javax.security.auth.x500.X500Principal;

/** 独立重新打开正式新建结果，核对实际主题、格式和密码；不进入正式资源。 */
public final class CreatedCertificateProbe {
    public static void main(String[] ignored) throws Exception {
        String[] request = Request.read(System.in);
        char[] storePass = System.getenv("SHIELD_READONLY_BOOTSTRAP_PASS").toCharArray();
        char[] keyPass = System.getenv("SHIELD_READONLY_FIXTURE_PASS").toCharArray();
        try {
            byte[] bytes = Files.readAllBytes(Paths.get(request[1]));
            boolean jks = bytes[0] == (byte) 0xfe && bytes[1] == (byte) 0xed
                && bytes[2] == (byte) 0xfe && bytes[3] == (byte) 0xed;
            if (jks != request[2].equals("JKS") || (!jks && bytes[0] != 48)) { throw new Exception(); }
            KeyStore store = KeyStore.getInstance(request[2]);
            try (InputStream input = Files.newInputStream(Paths.get(request[1]))) { store.load(input, storePass); }
            if (store.size() != 1 || !store.aliases().nextElement().equals(request[3])) { throw new Exception(); }
            X509Certificate certificate = (X509Certificate) store.getCertificate(request[3]);
            if (!certificate.getSubjectX500Principal().equals(new X500Principal(request[4]))) { throw new Exception(); }
            long duration = certificate.getNotAfter().getTime() - certificate.getNotBefore().getTime();
            if (duration != Long.parseLong(request[5]) * 86400000L
                    || ((java.security.interfaces.RSAPublicKey) certificate.getPublicKey()).getModulus().bitLength()
                        != Integer.parseInt(request[6])) { throw new Exception(); }
            certificate.verify(certificate.getPublicKey());
            PrivateKey key = (PrivateKey) store.getKey(request[3], keyPass);
            Signature proof = Signature.getInstance("SHA256withRSA");
            proof.initSign(key);
            proof.update(bytes);
            byte[] signature = proof.sign();
            proof.initVerify(certificate);
            proof.update(bytes);
            if (!proof.verify(signature)) { throw new Exception(); }
            try {
                store.getKey(request[3], "definitely-invalid-key-password".toCharArray());
                throw new Exception();
            } catch (UnrecoverableKeyException expected) { }
            System.out.print("{\"verified\":true}");
        } finally {
            Arrays.fill(storePass, '\0');
            Arrays.fill(keyPass, '\0');
        }
    }
}
