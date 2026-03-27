#!/bin/bash

# Comprehensive Integration Test Runner
# This script runs all integration tests with coverage and performance metrics

set -e

echo "🚀 Starting Comprehensive Integration Test Suite"
echo "================================================"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check if required tools are installed
check_dependencies() {
    echo -e "${BLUE}Checking dependencies...${NC}"
    
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}Error: cargo is not installed${NC}"
        exit 1
    fi
    
    if ! command -v psql &> /dev/null; then
        echo -e "${YELLOW}Warning: psql not found. Database tests may fail.${NC}"
    fi
    
    echo -e "${GREEN}✓ Dependencies check passed${NC}"
}

# Setup test environment
setup_test_env() {
    echo -e "${BLUE}Setting up test environment...${NC}"
    
    # Copy test environment file if it doesn't exist
    if [ ! -f .env.test ]; then
        if [ -f .env.example ]; then
            cp .env.example .env.test
            echo -e "${YELLOW}Created .env.test from .env.example${NC}"
        else
            echo -e "${RED}Error: No .env.example file found${NC}"
            exit 1
        fi
    fi
    
    # Set test database URL if not set
    if ! grep -q "TEST_DATABASE_URL" .env.test; then
        echo "TEST_DATABASE_URL=postgres://postgres:password@localhost/tipjar_test" >> .env.test
        echo -e "${YELLOW}Added TEST_DATABASE_URL to .env.test${NC}"
    fi
    
    echo -e "${GREEN}✓ Test environment setup complete${NC}"
}

# Run database migrations
run_migrations() {
    echo -e "${BLUE}Running database migrations...${NC}"
    
    # Check if sqlx-cli is installed
    if ! command -v sqlx &> /dev/null; then
        echo -e "${YELLOW}Installing sqlx-cli...${NC}"
        cargo install sqlx-cli --no-default-features --features postgres
    fi
    
    # Run migrations
    export DATABASE_URL=$(grep TEST_DATABASE_URL .env.test | cut -d '=' -f2)
    sqlx migrate run || echo -e "${YELLOW}Warning: Migration failed. Tests may not work properly.${NC}"
    
    echo -e "${GREEN}✓ Database migrations complete${NC}"
}

# Run individual test suites
run_test_suite() {
    local test_name=$1
    local test_file=$2
    
    echo -e "${BLUE}Running $test_name...${NC}"
    
    if cargo test --test "$test_file" -- --nocapture; then
        echo -e "${GREEN}✓ $test_name passed${NC}"
        return 0
    else
        echo -e "${RED}✗ $test_name failed${NC}"
        return 1
    fi
}

# Run all test suites
run_all_tests() {
    echo -e "${BLUE}Running all test suites...${NC}"
    
    local failed_tests=0
    local total_tests=0
    
    # Basic integration tests
    echo -e "\n${YELLOW}=== Basic Integration Tests ===${NC}"
    total_tests=$((total_tests + 1))
    run_test_suite "Tip Flow Tests" "tip_flows" || failed_tests=$((failed_tests + 1))
    
    total_tests=$((total_tests + 1))
    run_test_suite "Edge Case Tests" "edge_cases" || failed_tests=$((failed_tests + 1))
    
    total_tests=$((total_tests + 1))
    run_test_suite "Gas/Performance Tests" "gas_tests" || failed_tests=$((failed_tests + 1))
    
    # Advanced tests
    echo -e "\n${YELLOW}=== Advanced Integration Tests ===${NC}"
    total_tests=$((total_tests + 1))
    run_test_suite "Withdrawal Tests" "withdrawal_tests" || failed_tests=$((failed_tests + 1))
    
    total_tests=$((total_tests + 1))
    run_test_suite "Advanced Integration Tests" "advanced_integration_tests" || failed_tests=$((failed_tests + 1))
    
    # Comprehensive test runner
    echo -e "\n${YELLOW}=== Comprehensive Test Runner ===${NC}"
    total_tests=$((total_tests + 1))
    run_test_suite "Comprehensive Test Suite" "comprehensive_test_runner" || failed_tests=$((failed_tests + 1))
    
    # Summary
    echo -e "\n${BLUE}=== Test Summary ===${NC}"
    local passed_tests=$((total_tests - failed_tests))
    echo -e "Total Test Suites: $total_tests"
    echo -e "Passed: ${GREEN}$passed_tests${NC}"
    echo -e "Failed: ${RED}$failed_tests${NC}"
    
    if [ $failed_tests -eq 0 ]; then
        echo -e "${GREEN}🎉 All test suites passed!${NC}"
        return 0
    else
        echo -e "${RED}❌ $failed_tests test suite(s) failed${NC}"
        return 1
    fi
}

# Generate coverage report
generate_coverage() {
    echo -e "${BLUE}Generating coverage report...${NC}"
    
    if command -v cargo-tarpaulin &> /dev/null; then
        echo -e "${BLUE}Running tarpaulin for coverage...${NC}"
        cargo tarpaulin --out Html --output-dir coverage/ --tests || echo -e "${YELLOW}Warning: Coverage generation failed${NC}"
        
        if [ -f coverage/tarpaulin-report.html ]; then
            echo -e "${GREEN}✓ Coverage report generated: coverage/tarpaulin-report.html${NC}"
        fi
    else
        echo -e "${YELLOW}Tarpaulin not installed. Skipping coverage report.${NC}"
        echo -e "${YELLOW}Install with: cargo install cargo-tarpaulin${NC}"
    fi
}

# Run performance benchmarks
run_benchmarks() {
    echo -e "${BLUE}Running performance benchmarks...${NC}"
    
    if [ -d benches ]; then
        cargo bench || echo -e "${YELLOW}Warning: Benchmarks failed${NC}"
        echo -e "${GREEN}✓ Benchmarks complete${NC}"
    else
        echo -e "${YELLOW}No benchmark directory found. Skipping benchmarks.${NC}"
    fi
}

# Cleanup function
cleanup() {
    echo -e "${BLUE}Cleaning up...${NC}"
    # Add any cleanup tasks here
    echo -e "${GREEN}✓ Cleanup complete${NC}"
}

# Main execution
main() {
    echo -e "${GREEN}Starting comprehensive test suite...${NC}"
    
    # Setup
    check_dependencies
    setup_test_env
    run_migrations
    
    # Run tests
    if run_all_tests; then
        echo -e "\n${GREEN}🎉 All tests completed successfully!${NC}"
        
        # Optional: Generate coverage and run benchmarks
        if [ "$1" = "--full" ]; then
            generate_coverage
            run_benchmarks
        fi
        
        cleanup
        exit 0
    else
        echo -e "\n${RED}❌ Some tests failed${NC}"
        cleanup
        exit 1
    fi
}

# Handle script arguments
case "$1" in
    --help|-h)
        echo "Usage: $0 [--full] [--help]"
        echo ""
        echo "Options:"
        echo "  --full    Run full suite including coverage and benchmarks"
        echo "  --help    Show this help message"
        exit 0
        ;;
    *)
        main "$@"
        ;;
esac