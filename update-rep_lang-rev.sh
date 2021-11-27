find crates -type f -name Cargo.toml -print0 | xargs -0 sed -i "s/$1/$2/g"
