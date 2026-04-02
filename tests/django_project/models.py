"""Django models for gauntlet tests.

Models used to verify database isolation and benchmark ORM operations.
Covers: simple CRUD, ForeignKey, ManyToMany, aggregation, annotation queries.
"""

from django.db import models
from django.utils import timezone


class TestModel(models.Model):
    """Simple model for testing database isolation.

    Each test creates records with unique names. If isolation is working,
    records from one test should not be visible in other tests.
    """

    name = models.CharField(max_length=255, unique=True)
    value = models.IntegerField(default=0)
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        app_label = "django_project"

    def __str__(self):
        return f"TestModel({self.name}, {self.value})"


class TestUser(models.Model):
    """User model for testing user-related functionality."""

    name = models.CharField(max_length=255)
    email = models.EmailField(blank=True)

    class Meta:
        app_label = "django_project"

    def __str__(self):
        return f"TestUser({self.name})"


# ---------------------------------------------------------------------------
# Benchmark models -- exercise ForeignKey, M2M, aggregation, annotation
# ---------------------------------------------------------------------------


class Category(models.Model):
    """Product category with tree-like nesting via self-referential FK."""

    name = models.CharField(max_length=128)
    slug = models.SlugField(unique=True)
    parent = models.ForeignKey(
        "self", null=True, blank=True, on_delete=models.CASCADE, related_name="children"
    )

    class Meta:
        app_label = "django_project"
        verbose_name_plural = "categories"

    def __str__(self):
        return self.name


class Tag(models.Model):
    """Lightweight label for M2M benchmarks."""

    name = models.CharField(max_length=64, unique=True)

    class Meta:
        app_label = "django_project"

    def __str__(self):
        return self.name


class Product(models.Model):
    """Central benchmark model.  FK to Category, M2M to Tag."""

    name = models.CharField(max_length=255)
    sku = models.CharField(max_length=32, unique=True)
    price = models.DecimalField(max_digits=10, decimal_places=2)
    stock = models.PositiveIntegerField(default=0)
    is_active = models.BooleanField(default=True)
    category = models.ForeignKey(
        Category, on_delete=models.CASCADE, related_name="products"
    )
    tags = models.ManyToManyField(Tag, blank=True, related_name="products")
    created_at = models.DateTimeField(default=timezone.now)
    updated_at = models.DateTimeField(auto_now=True)
    description = models.TextField(blank=True, default="")

    class Meta:
        app_label = "django_project"
        indexes = [
            models.Index(fields=["sku"]),
            models.Index(fields=["price"]),
            models.Index(fields=["is_active", "stock"]),
        ]

    def __str__(self):
        return f"{self.name} ({self.sku})"


class Order(models.Model):
    """Order header -- FK to TestUser, date-based queries."""

    STATUS_CHOICES = [
        ("pending", "Pending"),
        ("confirmed", "Confirmed"),
        ("shipped", "Shipped"),
        ("delivered", "Delivered"),
        ("cancelled", "Cancelled"),
    ]

    customer_name = models.CharField(max_length=255)
    email = models.EmailField()
    status = models.CharField(max_length=16, choices=STATUS_CHOICES, default="pending")
    placed_at = models.DateTimeField(default=timezone.now)
    total = models.DecimalField(max_digits=12, decimal_places=2, default=0)
    notes = models.TextField(blank=True, default="")

    class Meta:
        app_label = "django_project"
        indexes = [
            models.Index(fields=["status"]),
            models.Index(fields=["placed_at"]),
        ]

    def __str__(self):
        return f"Order#{self.pk} ({self.status})"


class OrderItem(models.Model):
    """Line item -- FK to Order and Product, used in aggregation benchmarks."""

    order = models.ForeignKey(Order, on_delete=models.CASCADE, related_name="items")
    product = models.ForeignKey(
        Product, on_delete=models.CASCADE, related_name="order_items"
    )
    quantity = models.PositiveIntegerField(default=1)
    unit_price = models.DecimalField(max_digits=10, decimal_places=2)

    class Meta:
        app_label = "django_project"

    @property
    def line_total(self):
        return self.quantity * self.unit_price

    def __str__(self):
        return f"{self.product.name} x{self.quantity}"


class AuditLog(models.Model):
    """Write-heavy model for insert throughput benchmarks."""

    action = models.CharField(max_length=32)
    entity_type = models.CharField(max_length=64)
    entity_id = models.PositiveIntegerField()
    detail = models.TextField(blank=True, default="")
    created_at = models.DateTimeField(default=timezone.now)

    class Meta:
        app_label = "django_project"
        indexes = [
            models.Index(fields=["entity_type", "entity_id"]),
            models.Index(fields=["created_at"]),
        ]

    def __str__(self):
        return f"{self.action}({self.entity_type}#{self.entity_id})"
