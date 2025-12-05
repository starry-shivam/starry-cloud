# Use a small Python image
FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

# Create app directory
WORKDIR /app

# Install system deps (optional but nice to have)
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
  && rm -rf /var/lib/apt/lists/*

# Install Python dependencies
COPY requirements.txt .
# Add gunicorn for production serving
RUN pip install --no-cache-dir -r requirements.txt gunicorn

# Copy app code
COPY . .

# Default config path inside container
ENV STARRYCLOUD_CONFIG=/app/config.yml
ENV FLASK_ENV=production

EXPOSE 5000

# Run with gunicorn
CMD ["gunicorn", "--bind", "0.0.0.0:5000", "app:app", "--workers", "1", "--threads", "4"]