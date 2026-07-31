package dev.mocika.shield.smoke

import kotlin.coroutines.Continuation
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.coroutines.startCoroutine
import kotlin.coroutines.suspendCoroutine

/** 只用于验证 Kotlin 编译器生成方法与状态机的离线抽取、占位和恢复。 */
object KotlinDexSeparationCases {
    private var pending: Continuation<Int>? = null

    @JvmStatic
    @JvmOverloads
    fun defaultValue(prefix: String = "盾", count: Int = 4): String = "$prefix$count"

    @JvmStatic
    fun lambdaValue(input: Int): Int = listOf(input, input + 1).map { it * 2 }.sum()

    @JvmStatic
    fun syntheticValue(input: Int): Int = Holder(input).value + Holder(input + 2).value

    private data class Holder(val value: Int)

    private suspend fun suspendValue(input: Int): Int {
        val resumed = suspendCoroutine<Int> { continuation -> pending = continuation }
        return resumed * 2
    }

    @JvmStatic
    fun suspendValueBlocking(input: Int): Int {
        var output: Result<Int>? = null
        suspend { suspendValue(input) }.startCoroutine(object : Continuation<Int> {
            override val context = EmptyCoroutineContext

            override fun resumeWith(result: Result<Int>) {
                output = result
            }
        })
        pending?.resumeWith(Result.success(input + 3))
        pending = null
        return requireNotNull(output).getOrThrow()
    }
}
