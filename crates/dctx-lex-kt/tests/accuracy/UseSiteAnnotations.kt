package serialization

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Dto(
    @SerialName("user_id") val userId: Long,
    @field:Transient val cache: String? = null,
    @get:JvmName("isActiveFlag") val isActive: Boolean = false,
) {
    @Transient
    val derived = userId * 2

    @set:Inject
    lateinit var injected: String
}

@Serializable
enum class Status {
    @SerialName("on") ACTIVE,
    @SerialName("off") INACTIVE,
}
