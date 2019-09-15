package rustarok

import kotlinx.coroutines.*
import org.khronos.webgl.Uint8Array
import kotlin.js.Promise

external interface IDBFactory {

}

external interface IDBDatabase {

}

data class TextureEntry(val path: String, val hash: String, val count: Int)

object IndexedDb {
    private val indexedDb: IDBFactory =
            js("window.indexedDB || window.mozIndexedDB || window.webkitIndexedDB || window.msIndexedDB")

    suspend fun collect_mismatched_textures(entries: Map<String, DatabaseTextureEntry>): ArrayList<String> {
        val db = open()
        val tx = db.transaction("textures", "readonly")
        val store = tx.objectStore("textures")
        val mismatched_textures = arrayListOf<String>()
        for (entry in entries) {
            val client_entry: TextureEntry? = make_await { store.get(entry.key) }
            if (client_entry != null) {
                if (client_entry.count != entry.value.gl_textures.size ||
                        client_entry.hash != entry.value.hash) {
                    mismatched_textures.add(entry.key)
                }
            } else {
                mismatched_textures.add(entry.key)
            }
        }
        return mismatched_textures
    }

    private suspend fun open(): dynamic {
        val db = make_await<dynamic> {
            val req = indexedDb.asDynamic().open("rustarokDB")
            req.onupgradeneeded = { event: dynamic ->
                // Save the IDBDatabase interface
                val db: dynamic = event.target.result;

                val objectStore = db.createObjectStore("textures", object {
                    val keyPath = "path"
                })
                db.createObjectStore("texture_data")
                db.createObjectStore("models")
                db.createObjectStore("vertex_arrays")
                db.createObjectStore("effects")
            }
            req.onerror = { event: dynamic ->
                console.error("DB error: " + event.target.errorCode)
            }
            req
        }
        return db
    }

    suspend fun store_textures(path: String,
                               hash: String, count: Int,
                               textures: List<Triple<Int, Int, Uint8Array>>): Promise<Array<out dynamic>> {
        val db = open()
        val tx = db.transaction(arrayOf("textures", "texture_data"), "readwrite")
        val texture_info_store = tx.objectStore("textures")
        val texture_data_store = tx.objectStore("texture_data")
        val promises = textures.withIndex().map { (texture_index, texture) ->
            val key = "${path}_$texture_index"
            to_promise<dynamic> {
                texture_data_store.put(object {
                    val w = texture.first
                    val h = texture.second
                    val raw = texture.third
                }, key)
            }
        }.toTypedArray()
        return Promise.all(promises + to_promise {
            texture_info_store.put(object {
                val path = path
                val hash = hash
                val count = count
            })
        })
    }

    suspend fun store_model(path: String,
                            node_index: Int,
                            vertex_count: Int,
                            texture_name: String,
                            rawData: Uint8Array): Promise<dynamic> {
        val db = open()
        val tx = db.transaction("models", "readwrite")
        val store = tx.objectStore("models")
        val key = "${path}_$node_index"
        return to_promise<dynamic> {
            store.put(object {
                val vertex_count = vertex_count
                val texture_name = texture_name
                val raw = rawData
            }, key)
        }
    }

    suspend fun store_effect(name: String,
                             effect: StrFile): Promise<dynamic> {
        val db = open()
        val tx = db.transaction("effects", "readwrite")
        val store = tx.objectStore("effects")
        return to_promise<dynamic> {
            store.put(effect, name)
        }
    }


    suspend fun store_vertex_array(key: String, vertex_count: Int, rawData: Uint8Array): Promise<dynamic> {
        val db = open()
        val tx = db.transaction("vertex_arrays", "readwrite")
        val store = tx.objectStore("vertex_arrays")
        return to_promise<dynamic> {
            store.put(object {
                val vertex_count = vertex_count
                val raw = rawData
            }, key)
        }
    }

    suspend fun get_vertex_array(path: String): StoredVertexArray? {
        val db = open()
        val tx = db.transaction("vertex_arrays", "readonly")
        val store = tx.objectStore("vertex_arrays")
        val result = make_await<dynamic> { store.get(path) }
        return if (result != null) {
            StoredVertexArray(result)
        } else {
            null
        }
    }

    suspend fun get_effect(name: String): StrFile? {
        val db = open()
        val tx = db.transaction("effects", "readonly")
        val store = tx.objectStore("effects")
        val result = make_await<dynamic> { store.get(name) }
        return result
    }


    suspend fun get_model(path: String, i: Int): StoredModel? {
        val db = open()
        val tx = db.transaction("models", "readonly")
        val store = tx.objectStore("models")
        val result = make_await<dynamic> { store.get("${path}_$i") }
        return if (result != null) {
            StoredModel(result)
        } else {
            null
        }
    }

    suspend fun get_texture(path: String, i: Int): StoredTexture? {
        val db = open()
        val tx = db.transaction("texture_data", "readonly")
        val store = tx.objectStore("texture_data")
        val sh = make_await<dynamic> { store.get("${path}_$i") }
        return if (sh != null) {
            StoredTexture(sh)
        } else {
            null
        }
    }
}

fun <T> to_promise(block: () -> dynamic): Promise<T> {
    return Promise<T> { resolve, reject ->
        val req = block()
        req.onsuccess = {
            resolve(req.result)
        }
        req.onerror = { event: dynamic ->
            reject(event)
        }
    }.catch { e ->
        console.error(e)
        throw e
        0.asDynamic()
    }
}

suspend fun <T> make_await(block: () -> dynamic): T {
    return Promise<T> { resolve, reject ->
        val req = block()
        req.onsuccess = {
            resolve(req.result)
        }
        req.onerror = { event: dynamic ->
            reject(event)
        }
    }.catch { e ->
        console.error(e)
        throw e
        0.asDynamic()
    }.await()
}