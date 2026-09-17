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
            url: "https://github.com/cooklang/cooklang-find/releases/download/v0.7.1/CooklangFindFFI.xcframework.zip",
            checksum: "e3f473400eee75e62ece483fdf1a00765b87d0f2c442a2b8bc25ea1ee9cad22e"
        ),
    ]
)
