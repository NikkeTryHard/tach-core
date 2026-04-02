"""Django ORM benchmark suite for tach-core vs pytest-xdist comparison.

150+ tests exercising: bulk CRUD, FK traversal, M2M, aggregation,
annotation, subqueries, and write throughput. Each test is self-contained
and isolation-safe (savepoint rollback via conftest).
"""

import decimal
import pytest

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_category(name, slug, parent=None):
    from django_project.models import Category

    return Category.objects.create(name=name, slug=slug, parent=parent)


def _make_tag(name):
    from django_project.models import Tag

    return Tag.objects.create(name=name)


def _make_product(name, sku, price, stock, category, is_active=True):
    from django_project.models import Product

    return Product.objects.create(
        name=name,
        sku=sku,
        price=decimal.Decimal(str(price)),
        stock=stock,
        category=category,
        is_active=is_active,
    )


def _make_order(customer, email, status="pending"):
    from django_project.models import Order

    return Order.objects.create(customer_name=customer, email=email, status=status)


def _make_order_item(order, product, qty, unit_price):
    from django_project.models import OrderItem

    return OrderItem.objects.create(
        order=order,
        product=product,
        quantity=qty,
        unit_price=decimal.Decimal(str(unit_price)),
    )


def _make_audit(action, entity_type, entity_id, detail=""):
    from django_project.models import AuditLog

    return AuditLog.objects.create(
        action=action,
        entity_type=entity_type,
        entity_id=entity_id,
        detail=detail,
    )


def _seed_catalog(n_categories=5, n_products_per_cat=10, n_tags=8):
    """Seed a small product catalog. Returns (categories, products, tags)."""
    cats = [_make_category(f"cat_{i}", f"cat-{i}") for i in range(n_categories)]
    tags = [_make_tag(f"tag_{i}") for i in range(n_tags)]
    products = []
    idx = 0
    for cat in cats:
        for j in range(n_products_per_cat):
            p = _make_product(
                f"prod_{idx}",
                f"SKU-{idx:04d}",
                price=10.0 + idx * 0.5,
                stock=idx % 50,
                category=cat,
                is_active=(idx % 7 != 0),
            )
            p.tags.add(tags[idx % len(tags)])
            if idx % 3 == 0:
                p.tags.add(tags[(idx + 1) % len(tags)])
            products.append(p)
            idx += 1
    return cats, products, tags


def _seed_orders(products, n_orders=20, items_per_order=3):
    """Seed orders with line items referencing products."""
    from django_project.models import Order

    statuses = ["pending", "confirmed", "shipped", "delivered", "cancelled"]
    orders = []
    for i in range(n_orders):
        o = _make_order(
            f"customer_{i}",
            f"c{i}@example.com",
            status=statuses[i % len(statuses)],
        )
        total = decimal.Decimal("0")
        for j in range(items_per_order):
            p = products[(i * items_per_order + j) % len(products)]
            price = p.price
            _make_order_item(o, p, qty=1 + (j % 4), unit_price=price)
            total += price * (1 + (j % 4))
        o.total = total
        o.save(update_fields=["total"])
        orders.append(o)
    return orders


# ===========================================================================
# 1. CRUD basics (20 tests)
# ===========================================================================


class TestCrudOperations:
    def test_create_single_category(self):
        c = _make_category("Electronics", "electronics")
        assert c.pk is not None

    def test_create_nested_category(self):
        parent = _make_category("Electronics", "electronics")
        child = _make_category("Phones", "phones", parent=parent)
        assert child.parent_id == parent.pk

    def test_create_product_with_fk(self):
        cat = _make_category("Books", "books")
        p = _make_product("Python Cookbook", "BK-001", 49.99, 10, cat)
        assert p.category_id == cat.pk

    def test_bulk_create_products(self):
        from django_project.models import Product

        cat = _make_category("Bulk", "bulk")
        objs = [
            Product(
                name=f"item_{i}", sku=f"BULK-{i:04d}", price=i, stock=i, category=cat
            )
            for i in range(100)
        ]
        created = Product.objects.bulk_create(objs)
        assert len(created) == 100

    def test_update_product_stock(self):
        cat = _make_category("Update", "update")
        p = _make_product("Widget", "UPD-001", 5.0, 100, cat)
        p.stock = 50
        p.save(update_fields=["stock"])
        p.refresh_from_db()
        assert p.stock == 50

    def test_bulk_update_prices(self):
        from django_project.models import Product

        cat = _make_category("BulkUpd", "bulkupd")
        products = Product.objects.bulk_create(
            [
                Product(
                    name=f"bu_{i}", sku=f"BU-{i:04d}", price=10, stock=1, category=cat
                )
                for i in range(50)
            ]
        )
        for p in products:
            p.price = decimal.Decimal("99.99")
        Product.objects.bulk_update(products, ["price"])
        assert Product.objects.filter(price=decimal.Decimal("99.99")).count() == 50

    def test_delete_single(self):
        cat = _make_category("Del", "del")
        p = _make_product("Gone", "DEL-001", 1.0, 0, cat)
        pk = p.pk
        p.delete()
        from django_project.models import Product

        assert not Product.objects.filter(pk=pk).exists()

    def test_delete_cascade(self):
        from django_project.models import Product

        cat = _make_category("Cascade", "cascade")
        _make_product("Child1", "CAS-001", 1, 1, cat)
        _make_product("Child2", "CAS-002", 2, 2, cat)
        cat.delete()
        assert Product.objects.filter(sku__startswith="CAS-").count() == 0

    def test_get_or_create(self):
        from django_project.models import Tag

        tag, created = Tag.objects.get_or_create(name="new-tag")
        assert created
        tag2, created2 = Tag.objects.get_or_create(name="new-tag")
        assert not created2
        assert tag.pk == tag2.pk

    def test_update_or_create(self):
        from django_project.models import Tag

        tag, created = Tag.objects.update_or_create(
            name="uoc-tag", defaults={"name": "uoc-tag"}
        )
        assert created

    def test_create_order_with_items(self):
        cat = _make_category("OrdCat", "ordcat")
        p = _make_product("OrdProd", "ORD-001", 25.0, 5, cat)
        o = _make_order("Alice", "alice@test.com")
        item = _make_order_item(o, p, 3, 25.0)
        assert item.order_id == o.pk

    def test_audit_log_insert_throughput(self):
        from django_project.models import AuditLog

        logs = [
            AuditLog(
                action="create", entity_type="product", entity_id=i, detail=f"log {i}"
            )
            for i in range(200)
        ]
        AuditLog.objects.bulk_create(logs)
        assert AuditLog.objects.count() == 200

    def test_m2m_add_tags(self):
        cat = _make_category("M2M", "m2m")
        p = _make_product("Tagged", "M2M-001", 10, 1, cat)
        tags = [_make_tag(f"m2m_tag_{i}") for i in range(5)]
        p.tags.add(*tags)
        assert p.tags.count() == 5

    def test_m2m_remove_tags(self):
        cat = _make_category("M2MRem", "m2mrem")
        p = _make_product("UnTagged", "M2M-002", 10, 1, cat)
        tags = [_make_tag(f"rem_tag_{i}") for i in range(5)]
        p.tags.add(*tags)
        p.tags.remove(tags[0], tags[1])
        assert p.tags.count() == 3

    def test_m2m_clear(self):
        cat = _make_category("M2MClr", "m2mclr")
        p = _make_product("Cleared", "M2M-003", 10, 1, cat)
        p.tags.add(_make_tag("clr_tag_0"), _make_tag("clr_tag_1"))
        p.tags.clear()
        assert p.tags.count() == 0

    def test_m2m_set(self):
        cat = _make_category("M2MSet", "m2mset")
        p = _make_product("SetTags", "M2M-004", 10, 1, cat)
        t1, t2, t3 = _make_tag("set_0"), _make_tag("set_1"), _make_tag("set_2")
        p.tags.set([t1, t2])
        assert p.tags.count() == 2
        p.tags.set([t2, t3])
        assert set(p.tags.values_list("pk", flat=True)) == {t2.pk, t3.pk}

    def test_values_list(self):
        cat = _make_category("VL", "vl")
        for i in range(10):
            _make_product(f"vl_{i}", f"VL-{i:04d}", i, i, cat)
        from django_project.models import Product

        skus = list(
            Product.objects.filter(sku__startswith="VL-").values_list("sku", flat=True)
        )
        assert len(skus) == 10

    def test_only_defer(self):
        cat = _make_category("Defer", "defer")
        _make_product("Deferred", "DEF-001", 99, 5, cat, is_active=True)
        from django_project.models import Product

        p = Product.objects.only("name", "sku").get(sku="DEF-001")
        assert p.name == "Deferred"

    def test_exists_check(self):
        cat = _make_category("Exists", "exists")
        _make_product("Ex", "EX-001", 1, 1, cat)
        from django_project.models import Product

        assert Product.objects.filter(sku="EX-001").exists()
        assert not Product.objects.filter(sku="NOPE").exists()

    def test_count_vs_len(self):
        cat = _make_category("Count", "count")
        from django_project.models import Product

        Product.objects.bulk_create(
            [
                Product(
                    name=f"cnt_{i}", sku=f"CNT-{i:04d}", price=1, stock=1, category=cat
                )
                for i in range(30)
            ]
        )
        assert Product.objects.filter(sku__startswith="CNT-").count() == 30


# ===========================================================================
# 2. Query & filter benchmarks (30 tests)
# ===========================================================================


class TestQueryFilters:
    @pytest.fixture(autouse=True)
    def _catalog(self):
        self.cats, self.products, self.tags = _seed_catalog()

    def test_filter_by_category(self):
        from django_project.models import Product

        count = Product.objects.filter(category=self.cats[0]).count()
        assert count == 10

    def test_filter_active_products(self):
        from django_project.models import Product

        active = Product.objects.filter(is_active=True).count()
        assert active > 0

    def test_filter_price_range(self):
        from django_project.models import Product

        mid = Product.objects.filter(price__gte=20, price__lte=30).count()
        assert mid >= 0

    def test_filter_stock_zero(self):
        from django_project.models import Product

        out_of_stock = Product.objects.filter(stock=0).count()
        assert out_of_stock >= 0

    def test_exclude_inactive(self):
        from django_project.models import Product

        qs = Product.objects.exclude(is_active=False)
        assert qs.count() > 0

    def test_chained_filters(self):
        from django_project.models import Product

        qs = (
            Product.objects.filter(is_active=True)
            .filter(price__lt=30)
            .filter(stock__gt=5)
        )
        list(qs)  # force evaluation

    def test_q_objects_or(self):
        from django.db.models import Q
        from django_project.models import Product

        qs = Product.objects.filter(Q(stock=0) | Q(is_active=False))
        list(qs)

    def test_q_objects_complex(self):
        from django.db.models import Q
        from django_project.models import Product

        qs = Product.objects.filter(
            (Q(price__lt=15) & Q(stock__gt=10)) | Q(is_active=False)
        )
        list(qs)

    def test_icontains_lookup(self):
        from django_project.models import Product

        qs = Product.objects.filter(name__icontains="prod_1")
        assert qs.count() >= 1

    def test_startswith_lookup(self):
        from django_project.models import Product

        qs = Product.objects.filter(sku__startswith="SKU-00")
        assert qs.count() >= 1

    def test_in_lookup(self):
        from django_project.models import Product

        skus = [f"SKU-{i:04d}" for i in range(10)]
        qs = Product.objects.filter(sku__in=skus)
        assert qs.count() == 10

    def test_order_by_price(self):
        from django_project.models import Product

        prices = list(
            Product.objects.order_by("price").values_list("price", flat=True)[:10]
        )
        assert prices == sorted(prices)

    def test_order_by_desc(self):
        from django_project.models import Product

        prices = list(
            Product.objects.order_by("-price").values_list("price", flat=True)[:10]
        )
        assert prices == sorted(prices, reverse=True)

    def test_distinct_categories(self):
        from django_project.models import Product

        cats = Product.objects.values_list("category_id", flat=True).distinct()
        assert len(set(cats)) == 5

    def test_slice_pagination(self):
        from django_project.models import Product

        page1 = list(Product.objects.order_by("pk")[:10])
        page2 = list(Product.objects.order_by("pk")[10:20])
        assert len(page1) == 10
        assert len(page2) == 10
        assert page1[-1].pk < page2[0].pk

    def test_first_last(self):
        from django_project.models import Product

        first = Product.objects.order_by("pk").first()
        last = Product.objects.order_by("pk").last()
        assert first.pk < last.pk

    def test_filter_by_tag_m2m(self):
        from django_project.models import Product

        tagged = Product.objects.filter(tags__name="tag_0")
        assert tagged.count() >= 1

    def test_filter_by_multiple_tags(self):
        from django_project.models import Product

        qs = Product.objects.filter(tags__name__in=["tag_0", "tag_1"]).distinct()
        assert qs.count() >= 1

    def test_reverse_fk_lookup(self):
        from django_project.models import Category

        cat = Category.objects.prefetch_related("products").first()
        assert cat.products.count() >= 0

    def test_select_related_category(self):
        from django_project.models import Product

        p = Product.objects.select_related("category").first()
        assert p.category.name is not None

    def test_prefetch_related_tags(self):
        from django_project.models import Product

        products = list(Product.objects.prefetch_related("tags")[:20])
        for p in products:
            list(p.tags.all())  # should not hit DB again

    def test_annotate_tag_count(self):
        from django.db.models import Count
        from django_project.models import Product

        qs = Product.objects.annotate(tag_count=Count("tags")).filter(tag_count__gte=1)
        assert qs.count() >= 1

    def test_values_groupby(self):
        from django.db.models import Count
        from django_project.models import Product

        groups = list(
            Product.objects.values("category_id")
            .annotate(cnt=Count("id"))
            .order_by("category_id")
        )
        assert len(groups) == 5

    def test_raw_sql(self):
        from django_project.models import Product

        products = list(
            Product.objects.raw(
                "SELECT * FROM django_project_product WHERE stock > %s LIMIT 5", [0]
            )
        )
        assert isinstance(products, list)

    def test_iterator(self):
        from django_project.models import Product

        count = 0
        for p in Product.objects.iterator(chunk_size=10):
            count += 1
        assert count == 50

    def test_filter_none_parent(self):
        from django_project.models import Category

        roots = Category.objects.filter(parent__isnull=True)
        assert roots.count() == 5

    def test_double_underscore_traverse(self):
        from django_project.models import Product

        qs = Product.objects.filter(category__slug__startswith="cat-")
        assert qs.count() == 50

    def test_negated_q(self):
        from django.db.models import Q
        from django_project.models import Product

        qs = Product.objects.filter(~Q(stock=0))
        list(qs)

    def test_combined_select_prefetch(self):
        from django_project.models import Product

        qs = Product.objects.select_related("category").prefetch_related("tags")[:20]
        for p in qs:
            _ = p.category.name
            _ = list(p.tags.all())

    def test_subquery_latest_product(self):
        from django.db.models import Subquery, OuterRef
        from django_project.models import Product, Category

        latest = (
            Product.objects.filter(category=OuterRef("pk"))
            .order_by("-created_at")
            .values("name")[:1]
        )
        cats = Category.objects.annotate(latest_product=Subquery(latest))
        for c in cats:
            _ = c.latest_product


# ===========================================================================
# 3. Aggregation benchmarks (20 tests)
# ===========================================================================


class TestAggregations:
    @pytest.fixture(autouse=True)
    def _catalog_and_orders(self):
        self.cats, self.products, self.tags = _seed_catalog()
        self.orders = _seed_orders(self.products)

    def test_avg_price(self):
        from django.db.models import Avg
        from django_project.models import Product

        avg = Product.objects.aggregate(avg_price=Avg("price"))
        assert avg["avg_price"] is not None

    def test_sum_stock(self):
        from django.db.models import Sum
        from django_project.models import Product

        total = Product.objects.aggregate(total_stock=Sum("stock"))
        assert total["total_stock"] >= 0

    def test_min_max_price(self):
        from django.db.models import Min, Max
        from django_project.models import Product

        result = Product.objects.aggregate(min_p=Min("price"), max_p=Max("price"))
        assert result["min_p"] <= result["max_p"]

    def test_count_by_status(self):
        from django.db.models import Count
        from django_project.models import Order

        groups = list(
            Order.objects.values("status").annotate(cnt=Count("id")).order_by("status")
        )
        assert len(groups) >= 1

    def test_order_total_aggregate(self):
        from django.db.models import Sum
        from django_project.models import Order

        total = Order.objects.aggregate(grand_total=Sum("total"))
        assert total["grand_total"] > 0

    def test_avg_items_per_order(self):
        from django.db.models import Avg, Count
        from django_project.models import Order

        qs = Order.objects.annotate(item_count=Count("items")).aggregate(
            avg_items=Avg("item_count")
        )
        assert qs["avg_items"] > 0

    def test_top_products_by_order_count(self):
        from django.db.models import Count
        from django_project.models import Product

        top = list(
            Product.objects.annotate(order_count=Count("order_items")).order_by(
                "-order_count"
            )[:5]
        )
        assert len(top) <= 5

    def test_revenue_per_category(self):
        from django.db.models import Sum, F
        from django_project.models import OrderItem

        revenue = list(
            OrderItem.objects.values("product__category__name")
            .annotate(revenue=Sum(F("quantity") * F("unit_price")))
            .order_by("-revenue")
        )
        assert len(revenue) >= 1

    def test_annotate_with_expression(self):
        from django.db.models import F, Value, CharField
        from django.db.models.functions import Concat
        from django_project.models import Product

        qs = Product.objects.annotate(
            display=Concat(
                F("name"), Value(" ("), F("sku"), Value(")"), output_field=CharField()
            )
        )
        first = qs.first()
        assert "(" in first.display

    def test_conditional_aggregation(self):
        from django.db.models import Count, Q
        from django_project.models import Order

        result = Order.objects.aggregate(
            pending=Count("id", filter=Q(status="pending")),
            shipped=Count("id", filter=Q(status="shipped")),
        )
        assert result["pending"] >= 0

    def test_annotate_order_item_totals(self):
        from django.db.models import F, Sum
        from django_project.models import Order

        qs = Order.objects.annotate(
            computed_total=Sum(F("items__quantity") * F("items__unit_price"))
        )
        for o in qs:
            assert o.computed_total is not None or o.items.count() == 0

    def test_having_clause_via_filter(self):
        from django.db.models import Count
        from django_project.models import Category

        big_cats = Category.objects.annotate(product_count=Count("products")).filter(
            product_count__gte=5
        )
        assert big_cats.count() >= 1

    def test_stddev_price(self):
        from django.db.models import StdDev
        from django_project.models import Product

        result = Product.objects.aggregate(std=StdDev("price"))
        assert result["std"] is not None

    def test_variance_stock(self):
        from django.db.models import Variance
        from django_project.models import Product

        result = Product.objects.aggregate(var=Variance("stock"))
        assert result["var"] is not None

    def test_group_by_active(self):
        from django.db.models import Count, Avg
        from django_project.models import Product

        groups = list(
            Product.objects.values("is_active").annotate(
                cnt=Count("id"), avg_price=Avg("price")
            )
        )
        assert len(groups) == 2

    def test_date_trunc_orders(self):
        from django.db.models.functions import TruncDate
        from django.db.models import Count
        from django_project.models import Order

        by_date = list(
            Order.objects.annotate(date=TruncDate("placed_at"))
            .values("date")
            .annotate(cnt=Count("id"))
        )
        assert len(by_date) >= 1

    def test_coalesce_null_notes(self):
        from django.db.models.functions import Coalesce
        from django.db.models import Value, TextField
        from django_project.models import Order

        qs = Order.objects.annotate(
            safe_notes=Coalesce("notes", Value("(none)"), output_field=TextField())
        )
        for o in qs[:5]:
            assert o.safe_notes is not None

    def test_length_annotation(self):
        from django.db.models.functions import Length
        from django_project.models import Product

        qs = Product.objects.annotate(name_len=Length("name")).order_by("-name_len")
        first = qs.first()
        assert first.name_len == len(first.name)

    def test_case_when(self):
        from django.db.models import Case, When, Value, CharField
        from django_project.models import Product

        qs = Product.objects.annotate(
            stock_level=Case(
                When(stock=0, then=Value("out")),
                When(stock__lt=10, then=Value("low")),
                default=Value("ok"),
                output_field=CharField(),
            )
        )
        levels = set(qs.values_list("stock_level", flat=True))
        assert levels.issubset({"out", "low", "ok"})

    def test_f_expression_update(self):
        from django.db.models import F
        from django_project.models import Product

        Product.objects.filter(is_active=True).update(stock=F("stock") + 1)
        p = Product.objects.filter(is_active=True).first()
        assert p.stock >= 1


# ===========================================================================
# 4. Isolation proof tests (parametrized x20 -- prove rollback works)
# ===========================================================================


@pytest.mark.parametrize("iteration", range(20))
class TestIsolationProof:
    def test_product_table_empty(self, iteration):
        from django_project.models import Product

        assert Product.objects.count() == 0

    def test_order_table_empty(self, iteration):
        from django_project.models import Order

        assert Order.objects.count() == 0

    def test_create_then_isolated(self, iteration):
        cat = _make_category(f"iso_{iteration}", f"iso-{iteration}")
        _make_product(f"iso_prod_{iteration}", f"ISO-{iteration:04d}", 1, 1, cat)
        from django_project.models import Product

        assert Product.objects.count() == 1


# ===========================================================================
# 5. Write throughput (10 tests)
# ===========================================================================


class TestWriteThroughput:
    def test_insert_100_audit_logs(self):
        from django_project.models import AuditLog

        AuditLog.objects.bulk_create(
            [
                AuditLog(action="test", entity_type="benchmark", entity_id=i)
                for i in range(100)
            ]
        )
        assert AuditLog.objects.count() == 100

    def test_insert_500_audit_logs(self):
        from django_project.models import AuditLog

        AuditLog.objects.bulk_create(
            [
                AuditLog(action="bulk", entity_type="bench", entity_id=i)
                for i in range(500)
            ]
        )
        assert AuditLog.objects.count() == 500

    def test_mixed_crud_cycle(self):
        from django_project.models import AuditLog

        AuditLog.objects.bulk_create(
            [
                AuditLog(action="create", entity_type="cycle", entity_id=i)
                for i in range(50)
            ]
        )
        AuditLog.objects.filter(entity_id__lt=25).update(action="updated")
        AuditLog.objects.filter(entity_id__gte=25).delete()
        assert AuditLog.objects.count() == 25

    def test_sequential_creates(self):
        cat = _make_category("SeqCat", "seqcat")
        for i in range(50):
            _make_product(f"seq_{i}", f"SEQ-{i:04d}", i, i, cat)
        from django_project.models import Product

        assert Product.objects.filter(sku__startswith="SEQ-").count() == 50

    def test_order_with_many_items(self):
        cat = _make_category("BigOrder", "bigorder")
        products = [
            _make_product(f"bop_{i}", f"BOP-{i:04d}", 10 + i, 100, cat)
            for i in range(20)
        ]
        order = _make_order("BigBuyer", "big@test.com")
        for p in products:
            _make_order_item(order, p, qty=2, unit_price=p.price)
        assert order.items.count() == 20

    def test_tag_mass_assignment(self):
        cat = _make_category("TagMass", "tagmass")
        p = _make_product("ManyTags", "TM-001", 10, 1, cat)
        tags = [_make_tag(f"mass_{i}") for i in range(30)]
        p.tags.set(tags)
        assert p.tags.count() == 30

    def test_cascade_delete_order(self):
        cat = _make_category("CasDel", "casdel")
        products = [
            _make_product(f"cd_{i}", f"CD-{i:04d}", 5, 10, cat) for i in range(5)
        ]
        order = _make_order("DeleteMe", "del@test.com")
        order_pk = order.pk
        for p in products:
            _make_order_item(order, p, 1, p.price)
        order.delete()
        from django_project.models import OrderItem

        assert OrderItem.objects.filter(order_id=order_pk).count() == 0

    def test_update_all_prices(self):
        from django_project.models import Product

        cat = _make_category("UpAll", "upall")
        Product.objects.bulk_create(
            [
                Product(
                    name=f"ua_{i}", sku=f"UA-{i:04d}", price=10, stock=1, category=cat
                )
                for i in range(100)
            ]
        )
        updated = Product.objects.filter(sku__startswith="UA-").update(
            price=decimal.Decimal("1.00")
        )
        assert updated == 100

    def test_conditional_update(self):
        from django.db.models import Case, When, Value
        from django_project.models import Product

        cat = _make_category("CondUp", "condup")
        Product.objects.bulk_create(
            [
                Product(
                    name=f"cu_{i}", sku=f"CU-{i:04d}", price=10, stock=i, category=cat
                )
                for i in range(20)
            ]
        )
        Product.objects.filter(sku__startswith="CU-").update(
            is_active=Case(
                When(stock=0, then=Value(False)),
                default=Value(True),
            )
        )
        from django_project.models import Product as P

        assert P.objects.filter(sku="CU-0000", is_active=False).exists()

    def test_copy_products_to_new_category(self):
        from django_project.models import Product

        src = _make_category("Src", "src")
        dst = _make_category("Dst", "dst")
        Product.objects.bulk_create(
            [
                Product(
                    name=f"cp_{i}", sku=f"CP-{i:04d}", price=i, stock=i, category=src
                )
                for i in range(30)
            ]
        )
        for p in Product.objects.filter(category=src):
            p.pk = None
            p.sku = f"CPD-{p.sku}"
            p.category = dst
            p.save()
        assert Product.objects.filter(category=dst).count() == 30


# ===========================================================================
# 6. Edge cases & model validation (10 tests)
# ===========================================================================


class TestEdgeCases:
    def test_empty_queryset_aggregate(self):
        from django.db.models import Sum
        from django_project.models import Product

        result = Product.objects.aggregate(total=Sum("price"))
        assert result["total"] is None

    def test_empty_queryset_count(self):
        from django_project.models import Product

        assert Product.objects.count() == 0

    def test_none_parent_category(self):
        cat = _make_category("Root", "root")
        assert cat.parent is None

    def test_decimal_precision(self):
        cat = _make_category("Prec", "prec")
        p = _make_product("Precise", "PREC-001", "99.99", 1, cat)
        p.refresh_from_db()
        assert p.price == decimal.Decimal("99.99")

    def test_long_description(self):
        cat = _make_category("Long", "long")
        p = _make_product("LongDesc", "LONG-001", 1, 1, cat)
        p.description = "x" * 10000
        p.save()
        p.refresh_from_db()
        assert len(p.description) == 10000

    def test_unicode_name(self):
        cat = _make_category("Unicode", "unicode-cat")
        p = _make_product("Produkt", "UNI-001", 1, 1, cat)
        p.name = "Tsch\u00fcss Welt \u2013 Emoji\u2603"
        p.save()
        p.refresh_from_db()
        assert "\u2603" in p.name

    def test_zero_price(self):
        cat = _make_category("Free", "free")
        p = _make_product("Freebie", "FREE-001", 0, 100, cat)
        assert p.price == 0

    def test_max_stock(self):
        cat = _make_category("Max", "max")
        p = _make_product("MaxStock", "MAX-001", 1, 2**31 - 1, cat)
        p.refresh_from_db()
        assert p.stock == 2**31 - 1

    def test_ordering_with_nulls(self):
        from django_project.models import Category

        _make_category("A", "a-root")
        parent = _make_category("B", "b-root")
        _make_category("C", "c-child", parent=parent)
        qs = Category.objects.order_by("parent_id")
        list(qs)  # should not error

    def test_str_representations(self):
        cat = _make_category("StrCat", "strcat")
        p = _make_product("StrProd", "STR-001", 10, 5, cat)
        o = _make_order("StrCust", "str@test.com")
        assert "StrCat" in str(cat)
        assert "STR-001" in str(p)
        assert "pending" in str(o)
