package dev.mocika.shield.tools;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.AccessDeniedException;
import java.nio.file.NoSuchFileException;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.KeyStore;
import java.security.KeyStoreException;
import java.security.MessageDigest;
import java.security.UnrecoverableKeyException;
import java.security.cert.Certificate;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Enumeration;

/** 仅列举 Alias 与读取证书摘要；不调用 store、getKey 或任何证书写入 API。 */
final class CertificateReader {
    private static final String PASSWORD_ENV = "MOCIKA_SHIELD_TOOL_KS_PASS";

    static boolean supports(String[] request) {
        return (request.length == 3 && request[0].equals("keystore_aliases"))
            || (request.length == 4 && request[0].equals("keystore_fingerprint"));
    }

    static int run(String version, String[] request) {
        String secret = System.getenv(PASSWORD_ENV);
        if (secret == null) {
            return fail(version, request[0], "TOOL_FAILED");
        }
        char[] password = secret.toCharArray();
        secret = null;
        try {
            String type = request[2].isEmpty() ? KeyStore.getDefaultType() : request[2];
            if (!type.equalsIgnoreCase("JKS") && !type.equalsIgnoreCase("PKCS12")) {
                return fail(version, request[0], "FORMAT_INVALID");
            }
            KeyStore store = KeyStore.getInstance(type);
            try (InputStream input = Files.newInputStream(Paths.get(request[1]))) {
                store.load(input, password);
            }
            Reply reply = new Reply(version, request[0]);
            if (request[0].equals("keystore_aliases")) {
                ArrayList<String> names = new ArrayList<String>();
                Enumeration<String> aliases = store.aliases();
                while (aliases.hasMoreElements()) {
                    String name = aliases.nextElement();
                    if (names.size() >= 256 || name.getBytes(StandardCharsets.UTF_8).length > 4096) {
                        return fail(version, request[0], "RESULT_LIMIT");
                    }
                    names.add(name);
                }
                if (names.isEmpty()) {
                    return fail(version, request[0], "ALIAS_NOT_FOUND");
                }
                Collections.sort(names);
                reply.add(",\"result\":{\"aliases\":[");
                for (int i = 0; i < names.size(); i++) {
                    if (i > 0) { reply.add(","); }
                    reply.quote(names.get(i));
                }
                reply.add("]}");
            } else {
                if (!store.containsAlias(request[3])) {
                    return fail(version, request[0], "ALIAS_NOT_FOUND");
                }
                Certificate certificate = store.getCertificate(request[3]);
                if (certificate == null) {
                    return fail(version, request[0], "CERTIFICATE_MISSING");
                }
                byte[] digest = MessageDigest.getInstance("SHA-256").digest(certificate.getEncoded());
                StringBuilder hex = new StringBuilder();
                for (byte value : digest) {
                    hex.append(String.format(java.util.Locale.ROOT, "%02X", value & 255));
                }
                reply.add(",\"result\":{\"fingerprint\":").quote(hex.toString()).add("}");
            }
            System.out.print(reply.finish());
            return 0;
        } catch (NoSuchFileException error) {
            return fail(version, request[0], "FILE_NOT_FOUND");
        } catch (AccessDeniedException error) {
            return fail(version, request[0], "PERMISSION_DENIED");
        } catch (SecurityException error) {
            return fail(version, request[0], "PERMISSION_DENIED");
        } catch (IOException error) {
            Throwable cause = error;
            for (int i = 0; cause != null && i < 8; i++, cause = cause.getCause()) {
                if (cause instanceof UnrecoverableKeyException) {
                    return fail(version, request[0], "PASSWORD_INVALID");
                }
            }
            return fail(version, request[0], "FORMAT_INVALID");
        } catch (KeyStoreException error) {
            return fail(version, request[0], "FORMAT_INVALID");
        } catch (IllegalArgumentException error) {
            return fail(version, request[0], "RESULT_LIMIT");
        } catch (Exception error) {
            return fail(version, request[0], "TOOL_FAILED");
        } finally {
            Arrays.fill(password, '\0');
        }
    }

    private static int fail(String version, String operation, String code) {
        System.out.print(new Reply(version, operation).add(",\"error\":").quote(code).finish());
        return 4;
    }

    private CertificateReader() {}
}
