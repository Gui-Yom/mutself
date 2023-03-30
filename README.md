# mutself

Create self-mutating executables.

## Purpose

// TODO

## Usage

```rust
mutself::mutself! {
    DATA = "alice"
}

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        println!("Hello {}", &arg);
        mutself("new.exe", Some(&arg)).unwrap();
    } else {
        println!("Hello {}", &*DATA);
    }
}
```

Running our executable gives :

```shell
$ cargo run
Hello alice
```

Now with an argument :

```shell
$ cargo run -- bob
Hello bob
# This creates another executable named 'new.exe'
$ new
Hello bob
```

Yes, data size can be changed.

```shell
$ cargo run -- someverylongstringthatwouldntfitinsidethespace
```

## How ?

The library parses the executable it's running in and changes values before dumping a new executable.

## In depth How

Simply put : an abuse of the `#[link_section = ".custom"]` attribute. There is no assembly patching or anything. The
data entries in the `mutself!` macro are stored in a custom section in the executable. The code that run only has a
pointer to the top of the section (that pointer always stays valid because the custom section will always be at the
same place in virtual memory). All entries in the section are referenced relatively to this pointer.

The layout in memory is like so (factoring out alignment) :

```text
#ENTRY0 size (usize) <-- hardcoded pointer
#ENTRY0
#ENTRY1 size (usize)
#ENTRY1
// And so on
```

Since every entry is defined relatively to the base, they can grow or shrink in size freely without fear the runtime
code can't address them.
