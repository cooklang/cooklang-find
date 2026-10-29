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
            url: "https://github.com/cooklang/cooklang-find/releases/download/v0.12.1/CooklangFindFFI.xcframework.zip",
            checksum: "546ea36867085824f0827a05ba3c01e78f79e22fe23085303a35565715ccc601"
        ),
    ]
)
