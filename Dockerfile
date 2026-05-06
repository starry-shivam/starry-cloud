FROM python:3.12-alpine

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY config.yml .
COPY templates ./templates
COPY static ./static
COPY app.py .

EXPOSE 5000

CMD ["python", "app.py"]
