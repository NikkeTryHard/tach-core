#!/bin/bash
set -e

echo "🔥 Starting Phase 3 Integration Gauntlet..."

cd "$(dirname "$0")"

# 1. Setup environment
export PYTHONPATH=$(pwd)/.venv/lib/python3.12/site-packages:$(pwd)
export DJANGO_SETTINGS_MODULE=tests.django_project.settings
export PYO3_PYTHON=$(pwd)/.venv/bin/python

# 2. Ensure Django DB is ready
echo "📦 Setting up Django DB..."
python3 -c "import django; django.setup(); from django.core.management import call_command; call_command('migrate', '--run-syncdb', verbosity=0)" 2>/dev/null || true

# 3. Clean DB before run
echo "🧹 Cleaning DB..."
python3 -c "import django; django.setup(); from tests.django_project.models import TestUser; TestUser.objects.all().delete()"

# 4. Build release binary
echo "🔨 Building release binary..."
cargo build --release 2>/dev/null

# 5. Run Tach with Phase 3 gauntlet
echo "🚀 Running Phase 3 Gauntlet..."
sudo PYTHONHOME="" PYTHONPATH="$PYTHONPATH" DJANGO_SETTINGS_MODULE="$DJANGO_SETTINGS_MODULE" ./target/release/tach-core tests/gauntlet_phase3/

# 6. Verify DB Cleanup (Transaction Rollback worked?)
echo "🔍 Verifying DB Cleanup..."
COUNT=$(python3 -c "import django; django.setup(); from tests.django_project.models import TestUser; print(TestUser.objects.count())")

if [ "$COUNT" -eq "0" ]; then
    echo "✅ DB Clean (Rollback Successful)"
else
    echo "❌ DB Dirty! Found $COUNT records. Transaction Isolation Failed."
    exit 1
fi

echo ""
echo "🏆 Phase 3 Gauntlet PASSED!"
echo "   Async + DB + Env + Entropy + Isolation all work together."
echo ""
echo "   Ready to tag: v0.3.0-linux-complete"
