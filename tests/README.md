# 端到端回归测试

`fixtures/android-smoke-app` 是项目自有的最小 Android 测试夹具，只负责提供包含自定义 `Application` 和启动 `Activity` 的稳定输入 APK，不承载产品示例或业务功能。

`scripts/run-protect-e2e.sh` 负责无设备回归链路：编译测试 APK、临时生成测试证书、签名原始 APK、执行加固、再次签名，并验证签名、MSHD 块、壳 Native 库和 `check-apk` 结果。

```bash
make build-stub
bash tests/scripts/run-protect-e2e.sh
```

测试证书只在系统临时目录中生成，脚本退出时自动清理，不向仓库提交任何私钥。日常 CI 不启动模拟器；首次安装、清除数据、框架路由和真实运行时加载仍按发布测试清单在真机完成。
