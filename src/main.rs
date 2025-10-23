use clap::Parser;
use error_stack::{Report, ResultExt};

mod app_error;
mod client;
mod csv;
mod engine;
mod engine_error;
mod transaction;

/// Type aliasing to allow easier switchout of decimal type if needed.
type DecimalType = rust_decimal::Decimal;

/// The accuracy the input and ouput should be precise to.
const DECIMAL_ACCURACY: u32 = 4;

#[derive(Parser)]
#[command(version, about = "Toy Payments Engine")]
struct Args {
    /// Path to the CSV file
    csv_path: std::path::PathBuf,

    /// Enable verbose output, which currently equates to printing various soft client errors to stderr.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if let Err(report) = main_inner(&args, tokio::io::stdout()).await {
        eprintln!("{report:?}");
        std::process::exit(1);
    }
}

async fn main_inner(
    args: &Args,
    writer: impl tokio::io::AsyncWrite + Unpin,
) -> Result<(), Report<app_error::AppError>> {
    let mut engine = engine::spawn_engine(args.verbose);

    csv::process_input(
        &mut engine,
        tokio::fs::File::open(&args.csv_path)
            .await
            .change_context(app_error::AppError)?,
        args.verbose,
    )
    .await?;

    let engine_state = engine
        .shutdown()
        .await
        .attach("Shutting down engine failed")?;

    csv::output_client_state(engine_state.all_clients_state(), writer).await?;

    Ok(())
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use futures::StreamExt;
    use pretty_assertions::assert_eq;
    use rstest::*;

    use crate::{Args, csv::CsvOutputRecord, main_inner};

    /// Deserialize the output csv back into records for comparison during testing
    async fn output_csv_to_records(
        csv_contents: impl tokio::io::AsyncRead + Unpin + Send,
    ) -> Vec<CsvOutputRecord> {
        let mut reader = csv_async::AsyncReaderBuilder::new()
            .trim(csv_async::Trim::All)
            .create_deserializer(csv_contents);

        let mut records = reader.deserialize::<CsvOutputRecord>();
        let mut result = vec![];
        while let Some(record_result) = records.next().await {
            result.push(record_result.unwrap());
        }

        result
    }

    /// For each test case folder, read input.csv and expected.csv, process through the engine,
    /// and compare with the expected output. Client order does not matter when comparing.
    ///
    /// NOTE AI was used to help generate the testcases, which were then reviewed and augmented.
    #[rstest]
    #[case::brief_example("brief_example")]
    #[case::messy_input("messy_input")]
    #[case::dispute_holds_funds("dispute_holds_funds")]
    #[case::dispute_and_resolution("dispute_and_resolution")]
    #[case::dispute_and_chargeback("dispute_and_chargeback")]
    #[case::dispute_wrong_client_ignored("dispute_wrong_client_ignored")]
    #[case::resolve_wrong_client_ignored("resolve_wrong_client_ignored")]
    #[case::chargeback_wrong_client_ignored("chargeback_wrong_client_ignored")]
    #[case::dispute_nonexistent_tx_ignored("dispute_nonexistent_tx_ignored")]
    #[case::resolve_without_dispute_ignored("resolve_without_dispute_ignored")]
    #[case::chargeback_without_dispute_ignored("chargeback_without_dispute_ignored")]
    #[case::cannot_dispute_withdrawal("cannot_dispute_withdrawal")]
    #[case::double_dispute_ignored("double_dispute_ignored")]
    #[case::locked_account_ignores_transactions("locked_account_ignores_transactions")]
    #[case::insufficient_funds_withdrawal_fails("insufficient_funds_withdrawal_fails")]
    #[case::precision_four_decimal_places("precision_four_decimal_places")]
    #[case::resolve_already_resolved_ignored("resolve_already_resolved_ignored")]
    #[case::chargeback_already_resolved_ignored("chargeback_already_resolved_ignored")]
    #[case::multiple_disputes_different_transactions("multiple_disputes_different_transactions")]
    #[case::duplicate_tx_id_ignored("duplicate_tx_id_ignored")]
    #[case::duplicate_tx_id_different_client_ignored("duplicate_tx_id_different_client_ignored")]
    #[case::redispute_after_resolution("redispute_after_resolution")]
    #[case::cannot_redispute_after_chargeback("cannot_redispute_after_chargeback")]
    #[case::multiple_clients_complex("multiple_clients_complex")]
    #[case::held_funds_prevent_withdrawal("held_funds_prevent_withdrawal")]
    #[case::empty_input("empty_input")]
    #[case::client_created_on_first_transaction("client_created_on_first_transaction")]
    #[case::negative_amount_ignored("negative_amount_ignored")]
    #[case::zero_amount_transactions("zero_amount_transactions")]
    #[case::max_precision_exactly_four_places("max_precision_exactly_four_places")]
    #[case::precision_rounding_truncation("precision_rounding_truncation")]
    #[case::dispute_operations_ignore_amount("dispute_operations_ignore_amount")]
    #[case::max_client_id("max_client_id")]
    #[case::max_transaction_id("max_transaction_id")]
    #[case::resolve_no_client_exists("resolve_no_client_exists")]
    #[case::chargeback_no_client_exists("chargeback_no_client_exists")]
    #[case::withdrawal_creates_client_zero_balance("withdrawal_creates_client_zero_balance")]
    #[case::dispute_does_not_create_client("dispute_does_not_create_client")]
    #[case::interleaved_client_transactions("interleaved_client_transactions")]
    #[case::exact_balance_withdrawal("exact_balance_withdrawal")]
    #[case::operations_on_chargedback_tx_ignored("operations_on_chargedback_tx_ignored")]
    #[case::very_small_decimals("very_small_decimals")]
    #[case::dispute_zero_amount_deposit("dispute_zero_amount_deposit")]
    #[case::header_only("header_only")]
    #[case::dispute_after_withdrawal_negative_available(
        "dispute_after_withdrawal_negative_available"
    )]
    #[case::negative_available_prevents_withdrawal("negative_available_prevents_withdrawal")]
    #[case::chargeback_with_negative_available("chargeback_with_negative_available")]
    #[case::resolution_restores_from_negative("resolution_restores_from_negative")]
    #[tokio::test]
    async fn test_csv_inputs(#[case] test_case_name: &str) {
        let test_case_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_cases")
            .join(test_case_name);
        let csv_path = test_case_dir.join("input.csv");
        let expected_path = test_case_dir.join("expected.csv");

        let mut buf = vec![];
        main_inner(
            &Args {
                csv_path,
                verbose: false,
            },
            &mut buf,
        )
        .await
        .unwrap();

        let mut output_records = output_csv_to_records(std::io::Cursor::new(buf)).await;

        // Read expected output
        let expected_file = tokio::fs::File::open(&expected_path)
            .await
            .unwrap_or_else(|e| panic!("Failed to open {:?}: {}", expected_path, e));

        let mut expected_output_records = output_csv_to_records(expected_file).await;

        // Brief says ordering doesn't matter, for testing we'll order by client_id to compare:
        output_records.sort_by_key(|r| r.client_id());
        expected_output_records.sort_by_key(|r| r.client_id());

        assert_eq!(
            output_records, expected_output_records,
            "actual != expected for test case '{}'. CSV output records did not match expected.",
            test_case_name
        );
    }
}
