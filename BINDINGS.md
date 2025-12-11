# Platform Bindings

cooklang-find provides native bindings for iOS (Swift) and Android (Kotlin) through [UniFFI](https://mozilla.github.io/uniffi-rs/).

## Building Bindings

### Prerequisites

- Rust toolchain (install via [rustup](https://rustup.rs/))
- For iOS: macOS with Xcode
- For Android: Android NDK

### Quick Start

Generate bindings only (no cross-compilation):

```bash
# Swift bindings
./scripts/build-swift.sh --generate-only

# Kotlin bindings
./scripts/build-kotlin.sh --generate-only
```

Build for all platforms:

```bash
# Swift (requires macOS)
./scripts/build-swift.sh --all

# Kotlin/Android (requires Android NDK)
ANDROID_NDK_HOME=/path/to/ndk ./scripts/build-kotlin.sh --all
```

## Swift / iOS

### Installation

#### Swift Package Manager

Add to your `Package.swift`:

```swift
dependencies: [
    .package(url: "https://github.com/cooklang/cooklang-find", from: "0.5.1")
]
```

Or add via Xcode: File → Add Package Dependencies → Enter the repository URL.

#### Manual Installation

1. Download the latest release from GitHub
2. Drag `CooklangFind.xcframework` into your Xcode project
3. Add the Swift source files to your target

### Usage

```swift
import CooklangFind

// Load a recipe by name
do {
    let recipe = try getRecipe(baseDirs: ["./recipes"], name: "pancakes")
    print("Recipe: \(recipe.name() ?? "Unknown")")
    print("Servings: \(recipe.metadata().servings ?? 0)")

    // Get recipe content
    let content = try recipe.content()
    print(content)

    // Access step images
    let stepImages = recipe.stepImages()
    for image in stepImages.images {
        print("Section \(image.section), Step \(image.step): \(image.imagePath)")
    }
} catch let error as CooklangError {
    print("Error: \(error)")
}

// Search for recipes
let results = try search(baseDir: "./recipes", query: "chocolate")
for recipe in results {
    print("Found: \(recipe.name() ?? "Unknown")")
}

// Build a recipe tree
let tree = try buildTree(baseDir: "./recipes")
for node in tree.allNodes() {
    print("\(node.name) - hasRecipe: \(node.hasRecipe)")
}

// Create recipe from content
let content = """
---
title: Quick Omelette
servings: 2
---

Crack @eggs{3} into a bowl and whisk.
"""
let recipe = try recipeFromContent(content: content, name: nil)
print(recipe.name()) // "Quick Omelette"
```

### Error Handling

All functions that can fail throw `CooklangError`:

```swift
do {
    let recipe = try getRecipe(baseDirs: ["./recipes"], name: "nonexistent")
} catch let error as CooklangError.NotFound {
    print("Recipe not found: \(error.message)")
} catch let error as CooklangError.IoError {
    print("IO error: \(error.message)")
} catch {
    print("Other error: \(error)")
}
```

## Kotlin / Android

### Installation

#### Gradle (GitHub Packages)

Add to your `settings.gradle.kts`:

```kotlin
dependencyResolutionManagement {
    repositories {
        maven {
            url = uri("https://maven.pkg.github.com/cooklang/cooklang-find")
            credentials {
                username = System.getenv("GITHUB_ACTOR") ?: project.findProperty("gpr.user") as String?
                password = System.getenv("GITHUB_TOKEN") ?: project.findProperty("gpr.token") as String?
            }
        }
    }
}
```

Add to your `build.gradle.kts`:

```kotlin
dependencies {
    implementation("org.cooklang:cooklang-find:0.5.1")
}
```

#### Manual Installation

1. Download the latest `cooklang-find-android.zip` from GitHub releases
2. Extract and copy the `cooklang-find-android` module to your project
3. Add to your `settings.gradle.kts`:

```kotlin
include(":cooklang-find-android")
```

4. Add to your app's `build.gradle.kts`:

```kotlin
dependencies {
    implementation(project(":cooklang-find-android"))
}
```

### Usage

```kotlin
import uniffi.cooklang_find.*

// Load a recipe by name
try {
    val recipe = getRecipe(listOf("./recipes"), "pancakes")
    println("Recipe: ${recipe.name()}")
    println("Servings: ${recipe.metadata().servings}")

    // Get recipe content
    val content = recipe.content()
    println(content)

    // Access step images
    val stepImages = recipe.stepImages()
    stepImages.images.forEach { image ->
        println("Section ${image.section}, Step ${image.step}: ${image.imagePath}")
    }
} catch (e: CooklangException) {
    println("Error: ${e.message}")
}

// Search for recipes
val results = search("./recipes", "chocolate")
results.forEach { recipe ->
    println("Found: ${recipe.name()}")
}

// Build a recipe tree
val tree = buildTree("./recipes")
tree.allNodes().forEach { node ->
    println("${node.name} - hasRecipe: ${node.hasRecipe}")
}

// Create recipe from content
val content = """
---
title: Quick Omelette
servings: 2
---

Crack @eggs{3} into a bowl and whisk.
""".trimIndent()

val recipe = recipeFromContent(content, null)
println(recipe.name()) // "Quick Omelette"
```

### Error Handling

All functions that can fail throw `CooklangException`:

```kotlin
try {
    val recipe = getRecipe(listOf("./recipes"), "nonexistent")
} catch (e: CooklangException.NotFound) {
    println("Recipe not found: ${e.message}")
} catch (e: CooklangException.IoError) {
    println("IO error: ${e.message}")
} catch (e: CooklangException) {
    println("Other error: ${e.message}")
}
```

### ProGuard Rules

If you use ProGuard/R8, the AAR includes consumer rules. If needed manually:

```proguard
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.cooklang_find.** { *; }
```

## API Reference

### Functions

| Function | Description |
|----------|-------------|
| `getRecipe(baseDirs, name)` | Load a recipe by name from directories |
| `recipeFromContent(content, name)` | Create a recipe from string content |
| `recipeFromPath(path)` | Create a recipe from a file path |
| `search(baseDir, query)` | Search for recipes matching a query |
| `buildTree(baseDir)` | Build a hierarchical tree of recipes |
| `libraryVersion()` | Get the library version string |

### Types

#### FfiRecipeEntry

| Method | Returns | Description |
|--------|---------|-------------|
| `name()` | `String?` | Recipe name (from title or filename) |
| `path()` | `String?` | File path (if file-backed) |
| `fileName()` | `String?` | File name (if file-backed) |
| `content()` | `String` | Full recipe content |
| `metadata()` | `FfiMetadata` | Recipe metadata |
| `tags()` | `List<String>` | Recipe tags |
| `titleImage()` | `String?` | Title image path/URL |
| `stepImages()` | `FfiStepImages` | Step images |
| `isMenu()` | `Boolean` | Whether this is a .menu file |
| `getStepImage(section, step)` | `String?` | Get specific step image |
| `getMetadataValue(key)` | `String?` | Get metadata value as JSON |

#### FfiMetadata

| Field | Type | Description |
|-------|------|-------------|
| `title` | `String?` | Recipe title |
| `servings` | `Long?` | Number of servings |
| `tags` | `List<String>` | Recipe tags |
| `imageUrl` | `String?` | Primary image URL |
| `rawJson` | `String` | Full metadata as JSON |

#### FfiStepImages

| Field | Type | Description |
|-------|------|-------------|
| `images` | `List<StepImageEntry>` | All step images |
| `count` | `UInt` | Total image count |

#### StepImageEntry

| Field | Type | Description |
|-------|------|-------------|
| `section` | `UInt` | Section number (0 = linear) |
| `step` | `UInt` | Step number (1-indexed) |
| `imagePath` | `String` | Path to image |

#### FfiRecipeTree

| Method | Returns | Description |
|--------|---------|-------------|
| `root()` | `FfiTreeNode` | Root node |
| `allNodes()` | `List<FfiTreeNode>` | All nodes flattened |
| `allRecipes()` | `List<FfiRecipeEntry>` | All recipes |
| `getChild(name)` | `FfiTreeNode?` | Get child by name |
| `recipe()` | `FfiRecipeEntry?` | Recipe at root |
| `getRecipeAtPath(path)` | `FfiRecipeEntry?` | Get recipe by path |

#### FfiTreeNode

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Node name |
| `path` | `String` | Full path |
| `hasRecipe` | `Boolean` | Whether node has a recipe |
| `children` | `List<String>` | Child node names |

## CI/CD

The GitHub Actions workflow automatically:

1. Tests the Rust library on push/PR
2. Builds Swift XCFramework (arm64 device + arm64/x86_64 simulator)
3. Builds Android AAR with native libraries (arm64-v8a, armeabi-v7a, x86_64)
4. Creates GitHub Release with all artifacts
5. Updates `Package.swift` with correct checksum for SPM
6. Publishes Android AAR to GitHub Packages Maven repository

To create a release:

```bash
git tag v0.5.2
git push origin v0.5.2
```

This will trigger the workflow to build and publish:
- `CooklangFindFFI.xcframework.zip` - XCFramework binary (used by Package.swift)
- `CooklangFind-ios.zip` - Full iOS package with Swift sources
- `cooklang-find-android.zip` - Android library module with JNI libs
- Android AAR published to `maven.pkg.github.com/cooklang/cooklang-find`
