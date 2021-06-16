set -ex

main() {
    local tag=$(git tag --points-at HEAD)
    local src=$(pwd) \
          stage=

    if [ "$OS_NAME" = "macOS-latest" ]; then
        stage=$(mktemp -d -t tmp)
    else
        stage=$(mktemp -d)
    fi

    if [ "$OS_NAME" = "ubuntu-latest" ]; then
        cp target/$TARGET/release/livesplit_one $stage/.
    elif [ "$OS_NAME" = "macOS-latest" ]; then
        cp target/$TARGET/release/livesplit_one $stage/.
    elif [ "$OS_NAME" = "windows-latest" ]; then
        cp target/$TARGET/release/livesplit-one.exe $stage/.
    fi

    cd $stage
    if [ "$OS_NAME" = "windows-latest" ]; then
        7z a $src/livesplit-one-$tag-$TARGET.zip *
    else
        tar czf $src/livesplit-one-$tag-$TARGET.tar.gz *
    fi
    cd $src

    rm -rf $stage
}

main
