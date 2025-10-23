# Toy Payments Engine

MSRV: tested with `cargo-msrv` to be Rust `1.85.1`

A toy payments engine that processes transactions and events from csv input and outputs client account states.
Uses an event-driven architecture with async/await and tokio channels

## Architecture:
- An engine is spawned on a separate thread, returning an `engine::EngineHandle`. 
  This engine contains all client and transaction state.
- A csv is ingested row by row via a stream, each row is parsed into an `engine::EngineEvent`.
- Each `EngineEvent` is sent via a bounded channel to the engine event loop.
- After all events are processed, an `EngineEvent::Exit` event is sent, which triggers the engine to respond with it's `engine::EngineState` and shutdown.
- The engine state is used to serialize client account states to csv rows and streamed to stdout.

## Testing
End to end testing from csv input to expected output csv. Testcases defined with `rstest`, input/expected output csvs defined in the `test_cases` directory and loaded into the tests in `main.rs`. I used AI to help generate the various boilerplate testing scenarios, which I then reviewed and augmented.

## AI Usage
Only AI usage was to help generate the testcases (Claude Sonnet 4.5).

## Design Decisions
### Event based architecture
Brief mentions thousands of concurrent TCP streams, therefore chose an event based design with tokio channels.
- Decouples ingestion from processing, multiple ingestors can feed one engine
- Uses bounded channels for communication for backpressure to prevent unbounded memory growth under load
- Avoids locks/mutexes
- Ensures determinstic processing order

### Async (tokio) over sync channels
Originally considered `crossbeam` for channels with no async, but chose tokio/async due to networking future requirements mentioned. While a sync implementation may be slightly more efficient for the current scope, async tokio is more future proof to future needs.

### Negative balances
Withdrawals that would cause a user's `available` to go negative are rejected/ignored. However the `available` itself can go negative if the user has withdrawn funds that are later disputed. This is a realistic scenario where the client owes money back to the exchange.

### Decimal precision to 4 decimal places
Uses `rust_decimal` for 4dp precision. A fixed point `i64` solution could be slightly more efficient, `rust_decimal` is cleaner, more standard and maintainable. IO is likely the bottleneck anyway and microoptimisations without benchmarking should be avoided. Implemented via a `main::DecimalType` type alias to allow switching out for another backend later on.

### Client state datastructures
In both cases, opted for `HashMap<IdOfT, T>`. Client scope limited to `u16::MAX` so considered stack allocating an array, but this could lead to stack overflows, and creates large fixed memory usage for potentially only a few sparse ids. 
A global `HashSet<TxID>` is also used to ensure no duplicate transactions are processed across all clients.

### Error handling with `thiserror` and `error-stack`
`thiserror` for creating errors, `error-stack` for the `Report<E>` wrapper to provide rich error formatting, error locations etc.
- `engine_error::EngineError`: errors that can happen on the engine side, all but `EngineError::InternalError` are soft errors based on invalid client requests/csv rows, that will be printed to stderr when `-v` enabled, but otherwise ignored, a later version of the engine could make these soft errors available to the clients themselves, or some other controller depending on the logic flow.
- `app_error::AppError`: a top level catch-all error for anything that goes wrong during the main thread's logic flow. This error doesn't have any variants, as all errors should exit the program, emitting the formatted error to stderr.

### Transaction storage and memory growth
Transactions are stored indefinitely because:
- The brief doesn't mention possibility of expiry or completion states, therefore any normal transaction could be disputed
- The brief doesn't state a resolved transaction could not be disputed again
- Compliance could require retrieving historical transactions

Potential concern: the txid is a `u32`, meaning the transaction record store could in theory hold `u32::MAX` transactions. Chargebacks do allow potential cleanup, due to account locking, but compliance likely requires retention. This remains an open problem in the final implementation pending further requirements.

### Further assumptions
- Only deposits can be disputed: the spec only outlines deposits. Disputes will be seen as client errors and ignored.
- Resolved transactions can be redisputed: the spec does not state this is prohibited.
- Csv input parsing is insensitive to extra whitespace, uppercase types, uppercase headers and unrecognised record types.
- Negative amounts in inputs are rejected as client errors and ignored
- All balances start at 0
- Disputes/resolutions/chargebacks referencing the wrong client id for the given transaction id are rejected as client errors and ignored.
- Duplicate transaction ids are rejected and ignored, preventing replay attacks and other misuse