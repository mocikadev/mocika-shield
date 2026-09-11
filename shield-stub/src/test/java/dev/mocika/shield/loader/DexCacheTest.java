package dev.mocika.shield.loader;

import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TemporaryFolder;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.file.Files;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

public class DexCacheTest {
    private static final int SCHEMA = 1;

    @Rule
    public final TemporaryFolder temporaryFolder = new TemporaryFolder();

    @Test
    public void 完整只读缓存通过校验() throws Exception {
        Fixture fixture = createValidCache();

        assertTrue(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 字节被篡改后拒绝缓存() throws Exception {
        Fixture fixture = createValidCache();
        File dex = new File(fixture.directory, "c1.dex");
        assertTrue(dex.setWritable(true));
        write(dex, new byte[]{0x64, 0x65, 0x79});
        assertTrue(dex.setReadOnly());

        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 缺少DEX文件时拒绝缓存() throws Exception {
        Fixture fixture = createValidCache();
        assertTrue(new File(fixture.directory, "c1.dex").delete());

        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 存在多余文件时拒绝缓存() throws Exception {
        Fixture fixture = createValidCache();
        File extra = new File(fixture.directory, "unexpected");
        assertTrue(extra.createNewFile());

        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 缺少完成标记时拒绝缓存() throws Exception {
        Fixture fixture = createValidCache();
        assertTrue(new File(fixture.directory, ".done").delete());

        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 无效缓存删除失败时拒绝继续() {
        try {
            DexCache.removeInvalidCache(new DeleteFailureFile());
            fail("删除失败后仍继续执行");
        } catch (SecurityException error) {
            assertEquals("C11", error.getMessage());
        }
    }

    private Fixture createValidCache() throws Exception {
        File directory = temporaryFolder.newFolder("cache");
        File dex = new File(directory, "c1.dex");
        write(dex, new byte[]{0x64, 0x65, 0x78, 0x0a});
        String root = DexCache.calculateRoot(directory, SCHEMA, 1);
        assertTrue(dex.setReadOnly());
        File done = new File(directory, ".done");
        assertTrue(done.createNewFile());
        assertTrue(done.setReadOnly());
        return new Fixture(directory, new DexCache.Identity(SCHEMA, 1, root));
    }

    @Test
    public void 系统优化目录不影响重复缓存命中() throws Exception {
        Fixture fixture = createValidCache();
        assertTrue(DexCache.validate(fixture.directory, fixture.identity));
        File oat = new File(fixture.directory, "oat/arm64");
        assertTrue(oat.mkdirs());
        File optimized = new File(oat, "c1.odex");
        write(optimized, new byte[]{1, 2, 3});
        assertTrue(DexCache.validate(fixture.directory, fixture.identity));
        assertTrue(DexCache.validate(fixture.directory, fixture.identity));
        assertTrue(optimized.isFile());
        File dex = new File(fixture.directory, "c1.dex");
        assertTrue(dex.setWritable(true));
        write(dex, new byte[]{9});
        assertTrue(dex.setReadOnly());
        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 优化目录不能是普通文件() throws Exception {
        Fixture fixture = createValidCache();
        assertTrue(new File(fixture.directory, "oat").createNewFile());
        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 优化目录不能掩盖缺失DEX() throws Exception {
        Fixture fixture = createValidCache();
        assertTrue(new File(fixture.directory, "c1.dex").delete());
        assertTrue(new File(fixture.directory, "oat").mkdir());
        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 清理优化目录内部链接不删除外部文件() throws Exception {
        Fixture fixture = createValidCache();
        File outside = temporaryFolder.newFolder("outside");
        File kept = new File(outside, "保留文件");
        write(kept, new byte[]{1});
        File oat = new File(fixture.directory, "oat");
        assertTrue(oat.mkdir());
        Files.createSymbolicLink(new File(oat, "arm64").toPath(), outside.toPath());
        DexCache.removeInvalidCache(fixture.directory);
        assertTrue(kept.isFile());
        assertFalse(fixture.directory.exists());
    }

    @Test
    public void 优化目录存在时仍拒绝额外DEX() throws Exception {
        Fixture fixture = createValidCache();
        assertTrue(new File(fixture.directory, "oat").mkdir());
        assertTrue(new File(fixture.directory, "c2.dex").createNewFile());
        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
    }

    @Test
    public void 优化目录符号链接被拒绝且清理不删除外部文件() throws Exception {
        Fixture fixture = createValidCache();
        File outside = temporaryFolder.newFolder("outside");
        File kept = new File(outside, "保留文件");
        write(kept, new byte[]{1});
        Files.createSymbolicLink(new File(fixture.directory, "oat").toPath(), outside.toPath());
        assertFalse(DexCache.validate(fixture.directory, fixture.identity));
        DexCache.removeInvalidCache(fixture.directory);
        assertTrue(kept.isFile());
        assertFalse(fixture.directory.exists());
    }

    private static void write(File file, byte[] bytes) throws Exception {
        try (FileOutputStream stream = new FileOutputStream(file)) {
            stream.write(bytes);
        }
    }

    private static final class Fixture {
        final File directory;
        final DexCache.Identity identity;

        Fixture(File directory, DexCache.Identity identity) {
            this.directory = directory;
            this.identity = identity;
        }
    }

    private static final class DeleteFailureFile extends File {
        DeleteFailureFile() {
            super("无法删除的缓存");
        }

        @Override
        public boolean isDirectory() {
            return false;
        }

        @Override
        public boolean delete() {
            return false;
        }
    }
}
