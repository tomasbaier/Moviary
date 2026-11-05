package cz.filmoteka.app

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.StrictMode
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.Settings
import androidx.activity.result.ActivityResult
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileOutputStream

private const val VLC_PACKAGE = "org.videolan.vlc"

// Result codes of VLC for Android (VideoPlayerActivity).
private const val VLC_CONNECTION_FAILED = 2
private const val VLC_PLAYBACK_ERROR = 3
private const val VLC_HARDWARE_ACCELERATION_ERROR = 4
private const val VLC_VIDEO_TRACK_LOST = 5

@InvokeArg
class PlayArgs {
  lateinit var path: String
  /** Milliseconds, 0 = from the start. */
  var position: Long = 0
  var title: String? = null
}

@InvokeArg
class StorageArgs {
  var request: Boolean = false
}

@InvokeArg
class UriArgs {
  lateinit var uri: String
}

@InvokeArg
class LanguageArgs {
  var language: String = "cs"
}

/** Language of the messages; set by the app (Rust) when its language is chosen. */
private var english = false

/** The message in the app's language. */
private fun tr(cs: String, en: String) = if (english) en else cs

@InvokeArg
class FrameArgs {
  lateinit var path: String
  lateinit var target: String
}

/**
 * Android part of Moviary, called from Rust (src-tauri/src/mobile.rs):
 * VLC for Android, folder picker, access to all files and video thumbnails.
 *
 * Folders that are not readable as files (USB drives on most phones) are used through the
 * document provider: their tree URI is kept with a persistable permission, listed with
 * [listTree] and files are read with [openFile]. Files are then `content://` URIs.
 */
@TauriPlugin(
  permissions = [Permission(strings = [Manifest.permission.READ_EXTERNAL_STORAGE], alias = "storage")]
)
class FilmotekaPlugin(private val activity: Activity) : Plugin(activity) {
  init {
    // VLC gets a file:// URI so it reads the file itself and finds subtitles next to it.
    // Android 7+ forbids file:// URIs in intents by default.
    StrictMode.setVmPolicy(StrictMode.VmPolicy.Builder().build())
  }

  @Command
  fun setLanguage(invoke: Invoke) {
    english = invoke.parseArgs(LanguageArgs::class.java).language == "en"
    invoke.resolve()
  }

  // ---------- VLC ----------

  @Command
  fun checkVlc(invoke: Invoke) {
    val ret = JSObject()
    try {
      val info = activity.packageManager.getPackageInfo(VLC_PACKAGE, 0)
      ret.put("installed", true)
      ret.put("version", info.versionName)
    } catch (e: PackageManager.NameNotFoundException) {
      ret.put("installed", false)
    }
    invoke.resolve(ret)
  }

  @Command
  fun playVideo(invoke: Invoke) {
    val args = invoke.parseArgs(PlayArgs::class.java)
    val isUri = args.path.startsWith("content://")
    val uri = if (isUri) Uri.parse(args.path) else Uri.fromFile(File(args.path))
    val intent = Intent(Intent.ACTION_VIEW).apply {
      setPackage(VLC_PACKAGE)
      setDataAndType(uri, "video/*")
      if (isUri) addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      putExtra("title", args.title?.takeIf { it.isNotBlank() } ?: File(args.path).nameWithoutExtension)
      if (args.position > 0) {
        putExtra("from_start", false)
        putExtra("position", args.position)
      } else {
        putExtra("from_start", true)
      }
    }
    try {
      startActivityForResult(invoke, intent, "onVlcClosed")
    } catch (e: ActivityNotFoundException) {
      invoke.reject(tr("VLC pro Android není nainstalovaný.", "VLC for Android is not installed."))
    }
  }

  /** VLC returns the position and length (ms) when closed; neither when the video ended. */
  @ActivityCallback
  fun onVlcClosed(invoke: Invoke, result: ActivityResult) {
    val data = result.data
    val position = data?.getLongExtra("extra_position", 0L) ?: 0L
    val duration = data?.getLongExtra("extra_duration", 0L) ?: 0L
    val ret = JSObject()
    if (position > 0) ret.put("position", position)
    if (duration > 0) ret.put("duration", duration)
    ret.put("ended", result.resultCode == Activity.RESULT_OK && position <= 0 && duration <= 0)
    val error = when (result.resultCode) {
      VLC_CONNECTION_FAILED -> tr(
        "VLC nemůže soubor otevřít. Má VLC povolený přístup k souborům?",
        "VLC cannot open the file. Does VLC have access to files?"
      )
      VLC_PLAYBACK_ERROR -> tr("Při přehrávání ve VLC došlo k chybě.", "An error occurred during playback in VLC.")
      VLC_HARDWARE_ACCELERATION_ERROR -> tr("Chyba hardwarového dekódování ve VLC.", "Hardware decoding error in VLC.")
      VLC_VIDEO_TRACK_LOST -> tr("VLC ztratilo obraz videa.", "VLC lost the video picture.")
      else -> null
    }
    if (error != null) ret.put("error", error)
    invoke.resolve(ret)
  }

  @Command
  fun openVlcStore(invoke: Invoke) {
    val market = Intent(Intent.ACTION_VIEW, Uri.parse("market://details?id=$VLC_PACKAGE"))
    try {
      activity.startActivity(market)
    } catch (e: ActivityNotFoundException) {
      val web = Intent(Intent.ACTION_VIEW, Uri.parse("https://play.google.com/store/apps/details?id=$VLC_PACKAGE"))
      try {
        activity.startActivity(web)
      } catch (e: ActivityNotFoundException) {
        invoke.reject(tr("Google Play ani prohlížeč nejsou k dispozici.", "Neither Google Play nor a browser is available."))
        return
      }
    }
    invoke.resolve()
  }

  @Command
  fun openUrl(invoke: Invoke) {
    val uri = invoke.parseArgs(UriArgs::class.java).uri
    try {
      activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
      invoke.resolve()
    } catch (e: ActivityNotFoundException) {
      invoke.reject(tr("Není k dispozici žádný prohlížeč.", "No browser is available."))
    }
  }

  /** Opens the package installer for a downloaded update (an APK in the cache folder). */
  @Command
  fun installApk(invoke: Invoke) {
    val file = File(invoke.parseArgs(UriArgs::class.java).uri)
    try {
      val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
      val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, "application/vnd.android.package-archive")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      }
      activity.startActivity(intent)
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(tr("Instalaci nelze spustit: ${e.message}", "Cannot start the installation: ${e.message}"))
    }
  }

  // ---------- folders ----------

  @Command
  fun pickFolder(invoke: Invoke) {
    try {
      startActivityForResult(invoke, Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), "onFolderPicked")
    } catch (e: ActivityNotFoundException) {
      invoke.reject(tr("Výběr složky není v tomto zařízení k dispozici.", "Choosing a folder is not available on this device."))
    }
  }

  @ActivityCallback
  fun onFolderPicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    val ret = JSObject()
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      invoke.resolve(ret)
      return
    }
    // A folder readable as files is used by its path (faster; VLC finds subtitles next to the
    // video). Otherwise, typically on a USB drive, through the document provider. Without
    // the storage permission a listing would succeed but hide other apps' files.
    val path = treeToPath(uri)
    if (path != null && hasStorage() && File(path).list() != null) {
      ret.put("path", path)
      invoke.resolve(ret)
      return
    }
    try {
      activity.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
    } catch (e: SecurityException) {
      invoke.reject(tr("K této složce nelze získat trvalý přístup.", "Permanent access to this folder cannot be obtained."))
      return
    }
    ret.put("path", uri.toString())
    ret.put("label", treeLabel(uri))
    invoke.resolve(ret)
  }

  /** "SanDisk USB/Filmy" for a folder URI. */
  private fun treeLabel(uri: Uri): String {
    val docId = DocumentsContract.getTreeDocumentId(uri)
    val volume = docId.substringBefore(':')
    val rel = docId.substringAfter(':', "").trim('/')
    val manager = activity.getSystemService(StorageManager::class.java)
    val name = manager?.storageVolumes
      ?.firstOrNull { it.uuid.equals(volume, ignoreCase = true) }
      ?.getDescription(activity)
      ?: if (volume.equals("primary", ignoreCase = true)) tr("Úložiště", "Storage") else tr("Disk $volume", "Drive $volume")
    return if (rel.isEmpty()) name else "$name/$rel"
  }

  /** All files below a folder URI: rel (path inside the folder), size and uri. */
  @Command
  fun listTree(invoke: Invoke) {
    val args = invoke.parseArgs(UriArgs::class.java)
    Thread {
      try {
        val tree = Uri.parse(args.uri)
        val files = JSArray()
        listDocuments(tree, DocumentsContract.getTreeDocumentId(tree), "", files)
        val ret = JSObject()
        ret.put("files", files)
        invoke.resolve(ret)
      } catch (e: Exception) {
        invoke.reject(tr("Složka není dostupná: ${e.message}", "The folder is not available: ${e.message}"))
      }
    }.start()
  }

  private fun listDocuments(tree: Uri, parentId: String, prefix: String, out: JSArray) {
    val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId)
    val columns = arrayOf(
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE,
      DocumentsContract.Document.COLUMN_SIZE,
    )
    // A missing drive or a revoked permission throws, so the whole folder counts as offline.
    val cursor = activity.contentResolver.query(children, columns, null, null, null)
      ?: throw IllegalStateException("bez odpovědi")
    cursor.use {
      while (it.moveToNext()) {
        val id = it.getString(0)
        val name = it.getString(1) ?: continue
        val rel = if (prefix.isEmpty()) name else "$prefix/$name"
        if (it.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
          if (!(name.startsWith(".") || name.startsWith("$") || name.equals("System Volume Information", true))) {
            listDocuments(tree, id, rel, out)
          }
        } else {
          val file = JSObject()
          file.put("rel", rel)
          file.put("size", if (it.isNull(3)) 0L else it.getLong(3))
          file.put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, id).toString())
          out.put(file)
        }
      }
    }
  }

  /** Opens a content:// file for reading; Rust takes over the file descriptor. */
  @Command
  fun openFile(invoke: Invoke) {
    val args = invoke.parseArgs(UriArgs::class.java)
    try {
      val descriptor = activity.contentResolver.openFileDescriptor(Uri.parse(args.uri), "r")
      if (descriptor == null) {
        invoke.reject(tr("Soubor nelze otevřít.", "The file cannot be opened."))
        return
      }
      val ret = JSObject()
      ret.put("fd", descriptor.detachFd())
      invoke.resolve(ret)
    } catch (e: Exception) {
      invoke.reject(tr("Soubor nelze otevřít: ${e.message}", "The file cannot be opened: ${e.message}"))
    }
  }

  /** "content://…/tree/primary:Movies" -> "/storage/emulated/0/Movies". */
  private fun treeToPath(uri: Uri): String? {
    val docId = try {
      DocumentsContract.getTreeDocumentId(uri)
    } catch (e: Exception) {
      return null
    }
    when (uri.authority) {
      "com.android.externalstorage.documents" -> {
        val volume = docId.substringBefore(':')
        val rel = docId.substringAfter(':', "").trim('/')
        val base = when {
          volume.equals("primary", ignoreCase = true) -> Environment.getExternalStorageDirectory().path
          volume.equals("home", ignoreCase = true) ->
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS).path
          else -> volumeDirectory(volume) ?: "/storage/$volume"
        }
        return if (rel.isEmpty()) base else "$base/$rel"
      }
      "com.android.providers.downloads.documents" -> {
        if (docId.startsWith("raw:")) return docId.removePrefix("raw:")
        if (docId == "downloads") {
          return Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).path
        }
      }
    }
    return null
  }

  /** Mount point of an SD card or USB drive by its id ("1A2B-3C4D"). */
  private fun volumeDirectory(uuid: String): String? {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
    val manager = activity.getSystemService(StorageManager::class.java) ?: return null
    return manager.storageVolumes
      .firstOrNull { it.uuid.equals(uuid, ignoreCase = true) }
      ?.directory
      ?.path
  }

  // ---------- access to files ----------

  private fun hasStorage(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      Environment.isExternalStorageManager()
    } else {
      ContextCompat.checkSelfPermission(activity, Manifest.permission.READ_EXTERNAL_STORAGE) ==
        PackageManager.PERMISSION_GRANTED
    }

  private fun resolveStorage(invoke: Invoke) {
    val ret = JSObject()
    ret.put("granted", hasStorage())
    invoke.resolve(ret)
  }

  /** Android 11+: "Access to all files" in the system settings; older: the storage permission. */
  @Command
  fun storageAccess(invoke: Invoke) {
    val args = invoke.parseArgs(StorageArgs::class.java)
    if (hasStorage() || !args.request) {
      resolveStorage(invoke)
      return
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      val intent = Intent(
        Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
        Uri.parse("package:${activity.packageName}")
      )
      try {
        startActivityForResult(invoke, intent, "onStorageSettings")
      } catch (e: ActivityNotFoundException) {
        startActivityForResult(invoke, Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION), "onStorageSettings")
      }
    } else {
      requestPermissionForAlias("storage", invoke, "onStoragePermission")
    }
  }

  @ActivityCallback
  fun onStorageSettings(invoke: Invoke, result: ActivityResult) {
    resolveStorage(invoke)
  }

  @PermissionCallback
  fun onStoragePermission(invoke: Invoke) {
    resolveStorage(invoke)
  }

  // ---------- thumbnails ----------

  /** Saves a frame of the video as JPEG; tries a few positions because short videos end early. */
  @Command
  fun videoFrame(invoke: Invoke) {
    val args = invoke.parseArgs(FrameArgs::class.java)
    Thread {
      val ret = JSObject()
      ret.put("ok", saveFrame(args.path, File(args.target)))
      invoke.resolve(ret)
    }.start()
  }

  private fun saveFrame(path: String, target: File): Boolean {
    val retriever = MediaMetadataRetriever()
    try {
      if (path.startsWith("content://")) {
        retriever.setDataSource(activity, Uri.parse(path))
      } else {
        retriever.setDataSource(path)
      }
      val length = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0L
      val times = listOf(420_000L, 90_000L, 5_000L).filter { length <= 0 || it < length }
        .ifEmpty { listOf(length / 2) }
      for (ms in times) {
        val frame = retriever.getFrameAtTime(ms * 1000, MediaMetadataRetriever.OPTION_CLOSEST_SYNC) ?: continue
        val scaled = if (frame.width > 640) {
          Bitmap.createScaledBitmap(frame, 640, frame.height * 640 / frame.width, true)
        } else {
          frame
        }
        target.parentFile?.mkdirs()
        FileOutputStream(target).use { scaled.compress(Bitmap.CompressFormat.JPEG, 85, it) }
        return true
      }
    } catch (e: Exception) {
      // Unsupported format or unreadable file: no thumbnail.
    } finally {
      try {
        retriever.release()
      } catch (e: Exception) {
      }
    }
    return false
  }
}
