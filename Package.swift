// swift-tools-version:5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "CooklangFind",
    platforms: [
        .iOS(.v13),
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "CooklangFind",
            targets: ["CooklangFind", "CooklangFindFFI"]
        ),
    ],
    targets: [
        .target(
            name: "CooklangFind",
            dependencies: ["CooklangFindFFI"],
            path: "Sources/CooklangFind"
        ),
        .binaryTarget(
            name: "CooklangFindFFI",
            url: "https://github.com/cooklang/cooklang-find/releases/download/v0.5.1/CooklangFindFFI.xcframework.zip",
            checksum: "fb3cf1de6c4383967576b09138c0752009ce2714ce7e5a5edc4ea3081c008648"
        ),
    ]
)
